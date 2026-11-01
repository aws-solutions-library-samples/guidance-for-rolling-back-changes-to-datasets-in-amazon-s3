//! Rust build of `S3BatchOpsDeleteFunction`, the S3 Batch Operations Lambda function that adds and
//! removes delete markers (Scenarios 1, 2 and 4). It behaves like the Python function in the
//! CloudFormation templates:
//!
//!  - the key is URL-decoded as Python's `urllib.parse.unquote_plus` does it ('+' to ' ', %XX, lossy UTF-8)
//!  - with a version id, that version is deleted permanently (this removes a delete marker)
//!  - without a version id, `DeleteObject` runs with `If-Match: *` (this adds a delete marker)
//!  - NoSuchKey is a success ("Not found."); AccessDenied and any other error are permanent failures
//!  - the S3 client uses standard retry mode with 10 attempts
//!
//! The one visible difference is the generic error text, which reads "Retry Attempts = n/a": the Rust SDK
//! does not expose the retry count.

use aws_config::{retry::RetryConfig, BehaviorVersion};
use aws_sdk_s3::error::{ProvideErrorMetadata, SdkError};
use aws_sdk_s3::Client;
use lambda_runtime::{service_fn, Error, LambdaEvent};
use percent_encoding::percent_decode_str;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Event {
    invocation_schema_version: String,
    invocation_id: String,
    tasks: Vec<Task>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Task {
    task_id: String,
    s3_key: String,
    s3_version_id: Option<String>,
    // The Python function reads `s3Bucket`. Accept `s3BucketArn` too, which the schema also documents.
    s3_bucket: Option<String>,
    s3_bucket_arn: Option<String>,
}

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Response {
    invocation_schema_version: String,
    treat_missing_keys_as: &'static str,
    invocation_id: String,
    results: Vec<TaskResult>,
}

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
struct TaskResult {
    task_id: String,
    result_code: &'static str,
    result_string: String,
}

/// Equivalent of Python's `urllib.parse.unquote_plus`.
fn unquote_plus(s: &str) -> String {
    percent_decode_str(&s.replace('+', " ")).decode_utf8_lossy().into_owned()
}

fn bucket_of(task: &Task) -> String {
    if let Some(b) = task.s3_bucket.as_deref().filter(|b| !b.is_empty()) {
        return b.to_string();
    }
    task.s3_bucket_arn
        .as_deref()
        .map(|arn| arn.rsplit(':').next().unwrap_or(arn).to_string())
        .unwrap_or_default()
}

/// Maps an S3 error to the Batch Operations result, with the same strings as the Python function.
fn classify(code: &str, message: &str, status: u16, request_id: &str) -> (&'static str, String) {
    match code {
        "NoSuchKey" => ("Succeeded", "Not found.".to_string()),
        "AccessDenied" => ("PermanentFailure", format!("Access denied. RequestID = {request_id}.")),
        _ => (
            "PermanentFailure",
            format!("{code} - {message}. HTTP Status Code = {status}. Retry Attempts = n/a. RequestID = {request_id}."),
        ),
    }
}

async fn delete_one(client: &Client, task: Task) -> TaskResult {
    let bucket = bucket_of(&task);
    let key = unquote_plus(&task.s3_key);
    let version_id = task.s3_version_id.filter(|v| !v.is_empty());

    let req = client.delete_object().bucket(&bucket).key(&key);
    let outcome = match version_id {
        Some(v) => req.version_id(v).send().await,
        None => req.if_match("*").send().await,
    };

    let (result_code, result_string) = match outcome {
        Ok(_) => ("Succeeded", "Deleted".to_string()),
        Err(e) => {
            let status = match &e {
                SdkError::ServiceError(se) => se.raw().status().as_u16(),
                _ => 0,
            };
            classify(
                e.code().unwrap_or(""),
                e.message().unwrap_or(""),
                status,
                e.meta().extra("aws_request_id").unwrap_or(""),
            )
        }
    };
    TaskResult { task_id: task.task_id, result_code, result_string }
}

async fn handler(client: Client, event: LambdaEvent<Event>) -> Result<Response, Error> {
    let ev = event.payload;
    let mut results = Vec::with_capacity(ev.tasks.len());
    for task in ev.tasks {
        results.push(delete_one(&client, task).await);
    }
    Ok(Response {
        invocation_schema_version: ev.invocation_schema_version,
        treat_missing_keys_as: "PermanentFailure",
        invocation_id: ev.invocation_id,
        results,
    })
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Error> {
    let conf = aws_config::defaults(BehaviorVersion::latest())
        .retry_config(RetryConfig::standard().with_max_attempts(10))
        .load()
        .await;
    let client = Client::new(&conf);
    lambda_runtime::run(service_fn(move |ev| handler(client.clone(), ev))).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(json: &str) -> Task {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn unquote_plus_matches_python() {
        assert_eq!(unquote_plus("a+b%20c%2Fd"), "a b c/d");
        assert_eq!(unquote_plus("caf%C3%A9"), "café");
        assert_eq!(unquote_plus("100%25"), "100%");
        assert_eq!(unquote_plus("bad%ZZ"), "bad%ZZ");
        assert_eq!(unquote_plus("%FF"), "\u{FFFD}");
    }

    #[test]
    fn bucket_comes_from_s3_bucket_or_the_arn() {
        assert_eq!(bucket_of(&task(r#"{"taskId":"t","s3Key":"k","s3Bucket":"b1"}"#)), "b1");
        assert_eq!(bucket_of(&task(r#"{"taskId":"t","s3Key":"k","s3BucketArn":"arn:aws:s3:::b2"}"#)), "b2");
        assert_eq!(bucket_of(&task(r#"{"taskId":"t","s3Key":"k","s3Bucket":"","s3BucketArn":"arn:aws:s3:::b3"}"#)), "b3");
    }

    #[test]
    fn event_accepts_null_and_missing_version_ids() {
        let ev: Event = serde_json::from_str(
            r#"{"invocationSchemaVersion":"2.0","invocationId":"i","job":{"id":"j"},"tasks":[
                {"taskId":"a","s3Key":"k","s3Bucket":"b","s3VersionId":null},
                {"taskId":"b","s3Key":"k","s3Bucket":"b"},
                {"taskId":"c","s3Key":"k","s3Bucket":"b","s3VersionId":"V1"}]}"#,
        )
        .unwrap();
        let versions: Vec<_> = ev.tasks.iter().map(|t| t.s3_version_id.clone()).collect();
        assert_eq!(versions, vec![None, None, Some("V1".to_string())]);
    }

    #[test]
    fn error_classification_matches_the_python_function() {
        assert_eq!(classify("NoSuchKey", "m", 404, "R"), ("Succeeded", "Not found.".to_string()));
        assert_eq!(classify("AccessDenied", "m", 403, "R1"), ("PermanentFailure", "Access denied. RequestID = R1.".to_string()));
        assert_eq!(
            classify("KeyTooLongError", "Your key is too long", 400, "R2"),
            ("PermanentFailure", "KeyTooLongError - Your key is too long. HTTP Status Code = 400. Retry Attempts = n/a. RequestID = R2.".to_string())
        );
    }

    #[test]
    fn response_serialises_with_the_batch_operations_field_names() {
        let r = Response {
            invocation_schema_version: "2.0".into(),
            treat_missing_keys_as: "PermanentFailure",
            invocation_id: "i".into(),
            results: vec![TaskResult { task_id: "t".into(), result_code: "Succeeded", result_string: "Deleted".into() }],
        };
        assert_eq!(
            serde_json::to_value(&r).unwrap(),
            serde_json::json!({"invocationSchemaVersion":"2.0","treatMissingKeysAs":"PermanentFailure","invocationId":"i",
                "results":[{"taskId":"t","resultCode":"Succeeded","resultString":"Deleted"}]})
        );
    }
}
