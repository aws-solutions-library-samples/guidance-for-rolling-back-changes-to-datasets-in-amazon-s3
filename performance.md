# Lambda function performance and the optional Rust build
Measurements for the two Lambda functions that S3 Batch Operations invokes, and how to run the delete function as Rust. The [README](README.md#lambda-function-performance) has the summary.

## Contents
- [Delete function](#delete-function)
  - [Measured duration](#measured-duration)
  - [Why the function is Python](#why-the-function-is-python)
  - [What Rust would gain](#what-rust-would-gain)
  - [What limits job duration](#what-limits-job-duration)
- [Copy function](#copy-function)
  - [Memory](#memory)
  - [Threads and part size](#threads-and-part-size)
- [Deploying the delete function in Rust](#deploying-the-delete-function-in-rust)

## Delete function
S3 Batch Operations has no native delete operation, so Scenarios 1, 2 and 4 (adding and removing delete markers) invoke `S3BatchOpsDeleteFunction` once per object. Batch Operations sends one task per invocation. The function is Python 3.13 with 256 MB of memory, running on arm64.

### Measured duration
Each row is 10,000 deletes in eu-west-1 at 50 concurrent invocations, with keys spread across many prefixes. Duration excludes cold starts. Cost is the Lambda charge per million invocations, including the $0.20 per million request charge, and excludes S3 Batch Operations charges.

| Implementation | Memory | Median duration | 99th percentile | Cold start (init) | Lambda cost per million |
|---|---|---|---|---|---|
| **Python 3.13 (current)** | 256 MB | 28.2 ms | 116 ms | 441 ms | $0.32 |
| Rust (benchmark build) | 128 MB | 25.6 ms | 62 ms | 82 ms | $0.25 |
| Rust (benchmark build) | 256 MB | 25.6 ms | 55 ms | 78 ms | $0.30 |

Both functions run on arm64 (Graviton). x86_64 gave the same durations in these tests, with a lower price per GB-second on arm64: the Lambda charge is about 9% lower for the delete function. Memory above 256 MB did not reduce the warm duration of Python or Rust, and it raised the charge.

Most of the median duration is the S3 `DeleteObject` request itself: 24.6 ms of the 25.6 ms median for the Rust build.

### Why the function is Python
- CloudFormation can embed Python source in a template, so the tool deploys from the template alone, with no build step. A Rust or Go function is a compiled binary that must be uploaded to an S3 bucket in each Region where the template is deployed.
- The language does not set the duration of a job. The S3 request takes most of each invocation, and Batch Operations calls the function once per object.
- The gain from Rust is small, as shown below.

### What Rust would gain
The figures below come from a benchmark build of the function in [delete-function-rust](delete-function-rust), which has the same request logic plus optional timing code. Tested against the Python function, it showed:
- About 3 ms less per invocation (median 25.6 ms against 28.2 ms for Python on arm64), and an 80 ms cold start in place of 440 ms. Only about 1,000 invocations in a job are cold starts.
- A 17% to 37% lower Lambda charge in three paired jobs of 10 million tasks, which is about $9 to $17 per 100 million tasks. The Lambda request charge ($0.20 per million), the S3 Batch Operations charge ($1.00 per million objects) and the CloudWatch Logs charge do not change, so for 100 million tasks, which cost about $150 to $170 in total, the saving is about 5% to 11%.
- No expected change in how long a job takes. See [What limits job duration](#what-limits-job-duration) below: a function faster than about 100 ms reaches Lambda's request-rate limit whatever its language.

### What limits job duration
- Lambda. The requests-per-second quota is 10 times the concurrency quota: 10,000 per second at the default of 1,000 concurrent executions (981 in the test account, where other functions reserved 19). A function that takes less than about 100 ms reaches the request-rate limit before the concurrency limit, whatever its language. To measure this without S3, we ran Batch Operations jobs of 3 million tasks against a stand-in function that sleeps instead of calling S3:

  | Function duration | Invocations per second | 90% of tasks done after | All tasks done after | Throttled requests |
  |---|---|---|---|---|
  | 5 ms | 7,900 | 9 min | 19 min | 1.2 million |
  | 30 ms (two jobs) | 8,200 to 9,100 | 6 min | 12 to 13 min | 0.9 million |
  | 60 ms | 8,100 | 7 min | 12 min | 0.17 million |
  | 120 ms | 5,600 | 9 min | 9 min | 4,700 |
  | 250 ms | 3,300 | 16 min | 25 min | 39,000 |

  Batch Operations retries throttled requests, so none fail, but the retries stretch the end of a job: in five of the six jobs, 36% to 54% of the elapsed time went on the last 10% of tasks. The Python function (median 26 to 28 ms) and the Rust function (median 26 ms) are both in the range where the request-rate limit applies, so a faster function does not finish a job sooner. Above about 100 ms the function's duration sets the rate.
- S3. In jobs of 9 to 10 million real deletes, about 4% of delete requests were answered with a 5xx slow down response and retried by the function. The retry time is billed as Lambda duration, which is why Lambda cost per million ranged from $0.28 to $0.50 between jobs. The job rate followed what S3 accepted, and it rose from about 3,300 to 4,900 tasks per second across six successive jobs on the same bucket. A job of 100 million delete markers took 3.5 hours, about 7,900 tasks per second, within the 7,900 to 9,100 the stand-in function reached. At that rate the Lambda limit above is the constraint.

These figures come from tests in one account and Region on one bucket. Your results depend on the key layout and on the request rate S3 sustains for the prefixes in scope.

## Copy function
`S3BatchOpsCopyFunction` copies one object version per invocation with a managed S3 copy, using up to 1,000 threads and 250 MiB parts. It runs on Python 3.13 on arm64, with 1,024 MB of memory and a 900 s timeout.

In Bucket Rollback mode it runs only for Scenario 3c, which restores versions over 5 GB. Scenario 3b (overwritten versions up to 5 GB) does not use this function: it uses the native S3 Batch Operations copy. Copy to Bucket mode (Scenario 5b) uses the function for every object. The figures below are for versions over 5 GB.

Each row is server-side copies within eu-west-1 at 1,024 MB. Each copy ran on a new execution environment and was checked against its source. Cost is the Lambda charge for the run time, excluding the 0.4 to 0.5 s of initialization that a cold start also bills.

| Object size | Requests | Runs | Duration | Lambda cost per copy |
|---|---|---|---|---|
| 6 GB | 23 parts of 250 MiB, copied in parallel | 3 | 4.0 to 4.3 s | $0.000055 ($55 per million) |
| 50 GB | 191 parts | 3 | 11 to 17 s | $0.00015 to $0.00022 |
| 1 TB | 3,815 parts | 2 | 63 to 72 s | $0.00084 to $0.00096 |
| 5 TiB (the largest tested) | 5,243 parts of 1,000 MiB | 2 | 102 to 108 s | $0.0014 |

S3 allows objects up to 48.8 TiB. Versions over 5 TiB were not tested, so whether a copy of that size finishes within the 900 s timeout is not known. Duration did not scale in proportion to size: the 5 TiB copy took about 1.6 times as long as the 1 TB copy.

The 6 GB object is random data. The larger objects repeat a 5,000,000,000-byte random block; the copy runs inside S3, so the function never handles the data, but a 1 TB object of unique data was not tested. Duration depends on the object size and on S3: two copies of the same 50 GB object at 2,048 MB took 9.8 s and 29.1 s. x86_64 gave the same durations; arm64 lowers the Lambda charge by 22% to 27%.

### Memory
The same code ran at five memory sizes. Cost is for one 6 GB copy:

| MemorySize | 6 GB duration | 6 GB cost | 1 TB copy |
|---|---|---|---|
| 128 MB | 9.5 s | $0.000016 | timed out at 900 s |
| 256 MB | 6.2 s | $0.000021 | timed out at 900 s |
| 512 MB | 4.6 s | $0.000031 | 71 to 88 s |
| **1,024 MB (current)** | 4.1 s | $0.000055 | 63 to 72 s |
| 2,048 MB (previous) | 3.9 s | $0.000104 | 37 to 48 s |

- Peak memory depends on the number of parts copied in parallel, not on the bytes: 114 MB for 6 GB, about 170 MB for 50 GB, 230 to 450 MB for 1 TB and 5 TiB.
- 128 MB and 256 MB cannot copy a 1 TB version within the timeout.
- 512 MB copied every size, but used 80% of its memory on a 5 TiB object. The template uses 1,024 MB, which used at most 449 MB (44%) and costs 47% less than 2,048 MB for a 6 GB copy.

### Threads and part size
These runs used a 1 TB object at 512 MB, three runs each:

| Threads (`max_concurrency`) | Part size (`multipart_chunksize`) | Duration |
|---|---|---|
| 100 | 250 MiB | 178 to 198 s |
| 250 | 250 MiB | 86 to 92 s |
| 1,000 (current) | 250 MiB | 71 to 88 s (4 runs) |
| 1,000 | 128 MiB | 144 and 162 s; one of three runs failed |
| 1,000 | 512 MiB | 52 to 68 s |

- 1,000 threads did not fail or slow the copy, and used at most 319 MB.
- 512 MiB parts were faster on 1 TB and 50 GB (11 to 12 s against 14 to 17 s), and slower on a 6 GB version (7.4 s against 4.7 s), because a smaller object then has fewer parts to copy in parallel. The template keeps 250 MiB for the 5 GB to 50 GB versions that Scenario 3c copies most often.
- With 128 MiB parts, one run stopped with an SSL error after 333 s with 478 of 512 MB in use.

Each test ran one invocation at a time. Many copies running at once under Batch Operations were not tested.

## Deploying the delete function in Rust
The templates run the delete function as inline Python. You can run a compiled Rust build of the same function instead. Its source is in [delete-function-rust](delete-function-rust). The Rust function is optional and the Python function stays the default. See [Delete function](#delete-function) for what changes: a shorter duration per invocation and a lower Lambda charge, with the same behavior.

You need [Rust](https://www.rust-lang.org/tools/install), [Cargo Lambda](https://www.cargo-lambda.info/guide/installation.html) (which cross-compiles with Zig), and an S3 bucket in the same Region as the stack.
1. Build the function for arm64:
   ```
   cd delete-function-rust
   cargo lambda build --release --arm64 --output-format zip
   ```
   The zip file is `target/lambda/s3-rollback-delete-function/bootstrap.zip`.
2. Upload it to the bucket, using a new key for every build. CloudFormation does not redeploy a function whose bucket and key are unchanged:
   ```
   aws s3 cp target/lambda/s3-rollback-delete-function/bootstrap.zip s3://my-bucket/s3-rollback/delete-function-1.zip
   ```
3. Deploy `s3-rollback.yaml` or `s3-rollback-glue-metadata.yaml` and set both **Rust delete function code bucket** (`DeleteFunctionCodeBucket`, for example `my-bucket`) and **Rust delete function code key** (`DeleteFunctionCodeKey`, for example `s3-rollback/delete-function-1.zip`) under **Advanced: Rust delete function**. Set both, or leave both blank to use the Python function.

To change to a new build, upload it under a new key and update the stack with that key. To return to Python, update the stack with both parameters blank.

Notes:
- The function runs on the `provided.al2023` runtime on arm64 with 128 MB of memory. Build with `--arm64`: an x86_64 build fails when invoked.
- CloudFormation reads the zip file with the deploying principal's credentials, so that principal needs `s3:GetObject` on it. See [permissions.md](permissions.md#deploying-principal). The function's IAM role does not change.
- The Rust function returns the same results as the Python function. The one visible difference is the text for unexpected S3 errors, which ends "Retry Attempts = n/a".
- `s3-rollback-orchestrator.yaml` does not pass these parameters to its child stacks, so child stacks always use the Python function.
- This repository does not publish a built binary. You build and host the zip yourself, and the source is a single file of about 130 lines plus its unit tests.
