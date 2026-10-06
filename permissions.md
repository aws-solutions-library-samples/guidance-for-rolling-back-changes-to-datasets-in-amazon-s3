# IAM permissions

By default, each template creates the IAM roles it needs, scoped to the resources of that stack. If your organization creates IAM roles centrally, you can create them yourself and pass their ARNs to the templates instead. This page lists the permissions for both approaches, for each template.

The tool never reads or writes object ACLs.

## Contents

- [Placeholders](#placeholders)
- [Rollback template (`s3-rollback.yaml`)](#rollback-template-s3-rollbackyaml)
  - [Deploying principal](#deploying-principal)
  - [Pre-created shared role](#pre-created-shared-role)
  - [Lake Formation](#lake-formation)
  - [Options that need extra statements](#options-that-need-extra-statements)
- [Orchestrator (`s3-rollback-orchestrator.yaml`)](#orchestrator-s3-rollback-orchestratoryaml)
  - [Pre-created orchestrator roles](#pre-created-orchestrator-roles)
- [Organizations that restrict IAM role names](#organizations-that-restrict-iam-role-names)
- [Large-scale template (`s3-rollback-glue-metadata.yaml`)](#large-scale-template-s3-rollback-glue-metadatayaml)
- [KMS keys](#kms-keys)

## Placeholders

| Placeholder | Value |
|---|---|
| `PARTITION` | `aws` in commercial Regions |
| `REGION` | The Region you deploy in, which is the bucket's Region |
| `ACCOUNT_ID` | Your AWS account ID |
| `BUCKET` | The bucket to roll back or copy from |
| `NAMESPACE` | The bucket's S3 Metadata namespace, shown as `TableNamespace` by `aws s3api get-bucket-metadata-configuration --bucket BUCKET`. It is normally `b_` followed by the bucket name |
| `ROLE_NAME` | The name of the shared role you create. If the role has a path, include it without the leading `/`, for example `approved/rollback-shared` |
| `STACK_PREFIX` | A prefix that every stack name you deploy with this role starts with, for example `s3rb-`. Keep it to 20 characters or fewer: CloudFormation shortens long stack names when it generates bucket, function and role names, and the policy matches on the start of those names. Lowercase only, because S3 bucket names are lowercase |

Templates deployed with a pre-created role must use a stack name that starts with `STACK_PREFIX`.

## Rollback template (`s3-rollback.yaml`)

### Deploying principal

The principal that creates the stack needs the statements below. Use the first variant when the stack creates its own roles (the default), and the second when you pass `SharedIAMRoleArn`. Neither needs Lake Formation permissions: when the stack creates its own roles, one of them makes itself a Lake Formation administrator for the life of the stack.

Both variants assume the template is uploaded to `TEMPLATE_BUCKET`. Add `cloudformation:ListStacks` and `cloudformation:GetTemplateSummary` on `*` if you deploy from the CloudFormation console.

<!-- policy:deployer-common -->
```json
{
  "Version": "2012-10-17",
  "Statement": [
    {"Sid": "Stacks", "Effect": "Allow",
     "Action": ["cloudformation:CreateStack", "cloudformation:DeleteStack", "cloudformation:DescribeStacks",
                "cloudformation:DescribeStackEvents", "cloudformation:DescribeStackResources"],
     "Resource": "arn:PARTITION:cloudformation:REGION:ACCOUNT_ID:stack/STACK_PREFIX*/*"},
    {"Sid": "ValidateTemplate", "Effect": "Allow", "Action": "cloudformation:ValidateTemplate", "Resource": "*"},
    {"Sid": "ReadTemplate", "Effect": "Allow", "Action": "s3:GetObject", "Resource": "arn:PARTITION:s3:::TEMPLATE_BUCKET/*"},
    {"Sid": "ResultsBucket", "Effect": "Allow",
     "Action": ["s3:CreateBucket", "s3:DeleteBucket", "s3:PutBucketPublicAccessBlock", "s3:PutEncryptionConfiguration",
                "s3:GetEncryptionConfiguration", "s3:PutBucketPolicy", "s3:GetBucketPolicy", "s3:DeleteBucketPolicy",
                "s3:PutBucketTagging", "s3:GetBucketTagging"],
     "Resource": "arn:PARTITION:s3:::STACK_PREFIX*-athenaresultsbucket-*"},
    {"Sid": "Functions", "Effect": "Allow",
     "Action": ["lambda:CreateFunction", "lambda:DeleteFunction", "lambda:GetFunction", "lambda:GetFunctionConfiguration",
                "lambda:InvokeFunction", "lambda:TagResource", "lambda:UntagResource"],
     "Resource": "arn:PARTITION:lambda:REGION:ACCOUNT_ID:function:STACK_PREFIX*"},
    {"Sid": "FunctionLogGroups", "Effect": "Allow",
     "Action": ["logs:CreateLogGroup", "logs:TagResource", "logs:ListTagsForResource"],
     "Resource": "arn:PARTITION:logs:REGION:ACCOUNT_ID:log-group:/aws/lambda/s3-rollback/STACK_PREFIX*"},
    {"Sid": "DescribeLogGroups", "Effect": "Allow", "Action": "logs:DescribeLogGroups", "Resource": "*"},
    {"Sid": "AthenaWorkGroup", "Effect": "Allow",
     "Action": ["athena:CreateWorkGroup", "athena:DeleteWorkGroup", "athena:GetWorkGroup", "athena:UpdateWorkGroup",
                "athena:TagResource", "athena:CreateNamedQuery", "athena:DeleteNamedQuery", "athena:GetNamedQuery"],
     "Resource": "arn:PARTITION:athena:REGION:ACCOUNT_ID:workgroup/s3_rollback_wg_*"},
    {"Sid": "GlueDatabase", "Effect": "Allow",
     "Action": ["glue:CreateDatabase", "glue:DeleteDatabase", "glue:GetDatabase", "glue:GetTables", "glue:DeleteTable",
                "glue:GetUserDefinedFunctions"],
     "Resource": ["arn:PARTITION:glue:REGION:ACCOUNT_ID:catalog",
                  "arn:PARTITION:glue:REGION:ACCOUNT_ID:database/s3_rollback_db_*",
                  "arn:PARTITION:glue:REGION:ACCOUNT_ID:table/s3_rollback_db_*/*",
                  "arn:PARTITION:glue:REGION:ACCOUNT_ID:userDefinedFunction/s3_rollback_db_*/*"]}
  ]
}
```

Add this statement when the stack creates its own roles:

<!-- policy:deployer-own-roles -->
```json
{"Sid": "StackRoles", "Effect": "Allow",
 "Action": ["iam:CreateRole", "iam:DeleteRole", "iam:GetRole", "iam:PutRolePolicy", "iam:DeleteRolePolicy",
            "iam:GetRolePolicy", "iam:TagRole", "iam:UntagRole", "iam:PassRole"],
 "Resource": "arn:PARTITION:iam::ACCOUNT_ID:role/STACK_PREFIX*"}
```

Or this one when you pass `SharedIAMRoleArn`:

<!-- policy:deployer-shared-role -->
```json
{"Sid": "PassSharedRole", "Effect": "Allow", "Action": "iam:PassRole",
 "Resource": "arn:PARTITION:iam::ACCOUNT_ID:role/ROLE_NAME",
 "Condition": {"StringEquals": {"iam:PassedToService": "lambda.amazonaws.com"}}}
```

### Pre-created shared role

Pass the role's ARN as **Shared IAM role ARN** (`SharedIAMRoleArn`). Every Lambda function in the stack runs as this role, and every S3 Batch Operations job uses it, so the stack creates no IAM roles.

Trust policy:

<!-- policy:shared-trust -->
```json
{
  "Version": "2012-10-17",
  "Statement": [{"Effect": "Allow",
                 "Principal": {"Service": ["lambda.amazonaws.com", "batchoperations.s3.amazonaws.com"]},
                 "Action": "sts:AssumeRole"}]
}
```

Permissions policy, for a stack that reads S3 Metadata tables (the default inventory source):

<!-- policy:shared -->
```json
{
  "Version": "2012-10-17",
  "Statement": [
    {"Sid": "FunctionLogs", "Effect": "Allow", "Action": ["logs:CreateLogStream", "logs:PutLogEvents"],
     "Resource": "arn:PARTITION:logs:REGION:ACCOUNT_ID:log-group:/aws/lambda/s3-rollback/STACK_PREFIX*"},
    {"Sid": "SourceBucket", "Effect": "Allow",
     "Action": ["s3:ListBucket", "s3:GetBucketVersioning", "s3:GetBucketMetadataTableConfiguration", "s3:GetInventoryConfiguration"],
     "Resource": "arn:PARTITION:s3:::BUCKET"},
    {"Sid": "SourceObjects", "Effect": "Allow",
     "Action": ["s3:GetObject", "s3:GetObjectVersion", "s3:GetObjectTagging", "s3:GetObjectVersionTagging",
                "s3:PutObject", "s3:PutObjectTagging", "s3:DeleteObject", "s3:DeleteObjectVersion"],
     "Resource": "arn:PARTITION:s3:::BUCKET/*"},
    {"Sid": "ResultsBucket", "Effect": "Allow", "Action": ["s3:ListBucket", "s3:GetBucketLocation"],
     "Resource": "arn:PARTITION:s3:::STACK_PREFIX*-athenaresultsbucket-*"},
    {"Sid": "ResultsObjects", "Effect": "Allow", "Action": ["s3:GetObject", "s3:PutObject", "s3:DeleteObject"],
     "Resource": "arn:PARTITION:s3:::STACK_PREFIX*-athenaresultsbucket-*/*"},
    {"Sid": "CreateJobs", "Effect": "Allow", "Action": "s3:CreateJob", "Resource": "*"},
    {"Sid": "UpdateJobs", "Effect": "Allow", "Action": "s3:UpdateJobStatus",
     "Resource": "arn:PARTITION:s3:REGION:ACCOUNT_ID:job/*"},
    {"Sid": "PassSelfToBatchOperations", "Effect": "Allow", "Action": "iam:PassRole",
     "Resource": "arn:PARTITION:iam::ACCOUNT_ID:role/ROLE_NAME",
     "Condition": {"StringEquals": {"iam:PassedToService": "s3.amazonaws.com"}}},
    {"Sid": "InvokeStackFunctions", "Effect": "Allow", "Action": "lambda:InvokeFunction",
     "Resource": "arn:PARTITION:lambda:REGION:ACCOUNT_ID:function:STACK_PREFIX*"},
    {"Sid": "Athena", "Effect": "Allow",
     "Action": ["athena:StartQueryExecution", "athena:GetQueryExecution", "athena:GetQueryResults", "athena:GetNamedQuery"],
     "Resource": "arn:PARTITION:athena:REGION:ACCOUNT_ID:workgroup/s3_rollback_wg_*"},
    {"Sid": "AthenaS3TablesCatalog", "Effect": "Allow", "Action": "athena:GetDataCatalog",
     "Resource": "arn:PARTITION:athena:REGION:ACCOUNT_ID:datacatalog/s3tablescatalog"},
    {"Sid": "GlueStackDatabase", "Effect": "Allow",
     "Action": ["glue:GetDatabase", "glue:GetTable", "glue:GetTables", "glue:GetPartitions", "glue:CreateTable"],
     "Resource": ["arn:PARTITION:glue:REGION:ACCOUNT_ID:catalog",
                  "arn:PARTITION:glue:REGION:ACCOUNT_ID:database/s3_rollback_db_*",
                  "arn:PARTITION:glue:REGION:ACCOUNT_ID:table/s3_rollback_db_*/*"]},
    {"Sid": "GlueS3MetadataNamespace", "Effect": "Allow", "Action": ["glue:GetCatalog", "glue:GetDatabase", "glue:GetTable"],
     "Resource": ["arn:PARTITION:glue:REGION:ACCOUNT_ID:catalog",
                  "arn:PARTITION:glue:REGION:ACCOUNT_ID:catalog/s3tablescatalog",
                  "arn:PARTITION:glue:REGION:ACCOUNT_ID:catalog/s3tablescatalog/aws-s3",
                  "arn:PARTITION:glue:REGION:ACCOUNT_ID:database/s3tablescatalog/aws-s3/NAMESPACE",
                  "arn:PARTITION:glue:REGION:ACCOUNT_ID:table/s3tablescatalog/aws-s3/NAMESPACE/*"]},
    {"Sid": "LakeFormation", "Effect": "Allow",
     "Action": ["lakeformation:GetDataLakeSettings", "lakeformation:GetDataAccess",
                "lakeformation:GrantPermissions", "lakeformation:RevokePermissions"],
     "Resource": "*"},
    {"Sid": "S3MetadataTables", "Effect": "Allow",
     "Action": ["s3tables:GetTableBucket", "s3tables:GetNamespace", "s3tables:ListNamespaces", "s3tables:GetTable",
                "s3tables:ListTables", "s3tables:GetTableData", "s3tables:GetTableMetadataLocation", "s3tables:GetTableEncryption"],
     "Resource": ["arn:PARTITION:s3tables:REGION:ACCOUNT_ID:bucket/aws-s3",
                  "arn:PARTITION:s3tables:REGION:ACCOUNT_ID:bucket/aws-s3/table/*"]}
  ]
}
```

`s3:CreateJob` and the Lake Formation actions have no resource-level permissions, so they use `*`. To use one role for several buckets, list each bucket and namespace, or use `BUCKET` = `*` and `NAMESPACE` = `b_*`.

### Lake Formation

When the account's Glue Data Catalog or the S3 Tables catalog (`s3tablescatalog`) uses Lake Formation permissions, the stack grants Lake Formation permissions on its own Glue database and on the bucket's S3 Metadata namespace. With a pre-created shared role, the stack does not make any role a Lake Formation administrator. Choose one of these:

- **Make the shared role a data lake administrator** in the Region. No other change is needed.
- **Pass a separate administrator role** as **Lake Formation admin role ARN** (`LFAdminRoleArn`). The stack assumes it only to make and revoke the grants, so the shared role, which can delete object versions, stays off the administrator list. Add this statement to the shared role:

<!-- policy:shared-assume-lf-admin -->
```json
{"Sid": "AssumeLakeFormationAdmin", "Effect": "Allow", "Action": "sts:AssumeRole",
 "Resource": "arn:PARTITION:iam::ACCOUNT_ID:role/LF_ADMIN_ROLE_NAME"}
```

The administrator role must be a data lake administrator in the Region, trust the shared role, and have this policy:

<!-- policy:lf-admin-trust -->
```json
{
  "Version": "2012-10-17",
  "Statement": [{"Effect": "Allow", "Principal": {"AWS": "arn:PARTITION:iam::ACCOUNT_ID:root"}, "Action": "sts:AssumeRole",
                 "Condition": {"ArnLike": {"aws:PrincipalArn": "arn:PARTITION:iam::ACCOUNT_ID:role/ROLE_NAME"}}}]
}
```

<!-- policy:lf-admin -->
```json
{
  "Version": "2012-10-17",
  "Statement": [
    {"Sid": "LakeFormation", "Effect": "Allow",
     "Action": ["lakeformation:GetDataLakeSettings", "lakeformation:GrantPermissions",
                "lakeformation:RevokePermissions", "lakeformation:GetDataAccess"],
     "Resource": "*"},
    {"Sid": "Glue", "Effect": "Allow", "Action": ["glue:GetCatalog", "glue:GetDatabase", "glue:GetTable"],
     "Resource": ["arn:PARTITION:glue:REGION:ACCOUNT_ID:catalog",
                  "arn:PARTITION:glue:REGION:ACCOUNT_ID:catalog/s3tablescatalog",
                  "arn:PARTITION:glue:REGION:ACCOUNT_ID:catalog/s3tablescatalog/aws-s3",
                  "arn:PARTITION:glue:REGION:ACCOUNT_ID:database/s3tablescatalog/aws-s3/NAMESPACE",
                  "arn:PARTITION:glue:REGION:ACCOUNT_ID:table/s3tablescatalog/aws-s3/NAMESPACE/*",
                  "arn:PARTITION:glue:REGION:ACCOUNT_ID:database/s3_rollback_db_*",
                  "arn:PARTITION:glue:REGION:ACCOUNT_ID:table/s3_rollback_db_*/*"]},
    {"Sid": "S3Tables", "Effect": "Allow",
     "Action": ["s3tables:GetTableBucket", "s3tables:GetNamespace", "s3tables:ListNamespaces", "s3tables:GetTable", "s3tables:ListTables"],
     "Resource": ["arn:PARTITION:s3tables:REGION:ACCOUNT_ID:bucket/aws-s3",
                  "arn:PARTITION:s3tables:REGION:ACCOUNT_ID:bucket/aws-s3/table/*"]}
  ]
}
```

You can also pass `LFAdminRoleArn` without a shared role. The stack then creates its own roles but does not change the administrator list. In the trust policy, match the stack's role names instead of `ROLE_NAME`: `arn:PARTITION:iam::ACCOUNT_ID:role/STACK_PREFIX*`.

If a grant fails, the stack reports which role made it and what it was missing. Stack deletion always completes, even if a grant cannot be revoked; the stack's log group for `LFPermissionsGranter` then lists the grants that remain.

### Options that need extra statements

Add these to the shared role when you use the option.

| Option | Statements |
|---|---|
| **Copy to Bucket** mode | `s3:GetBucketVersioning` on `arn:PARTITION:s3:::DESTINATION`; `s3:PutObject` and `s3:PutObjectTagging` on `arn:PARTITION:s3:::DESTINATION/*`. The destination can be in any Region. If it is in another account, its bucket policy, and its KMS key policy if it uses one, must also allow the role |
| S3 Inventory as the inventory source | `s3:ListBucket` on the inventory destination bucket; `s3:GetObject` on its objects |
| **Specify CSV inventory** | `s3:ListBucket` on the CSV's bucket; `s3:GetObject` on the CSV |
| **Copy using KMS key** | See [KMS keys](#kms-keys) |
| **S3 Metadata tables KMS key** | See [KMS keys](#kms-keys) |

## Orchestrator (`s3-rollback-orchestrator.yaml`)

`ORCH_STACK` is the orchestrator's stack name. Child stacks are named `s3-rollback-...`, and CloudFormation creates them as the orchestrator's Lambda role.

By default the orchestrator creates two roles of its own, `LambdaExecutionRole` and `StepFunctionsExecutionRole`, and makes `LambdaExecutionRole` a Lake Formation administrator. Its deploying principal then needs IAM role creation (`iam:CreateRole`, `iam:PutRolePolicy`, `iam:DeleteRole`, `iam:DeleteRolePolicy`, `iam:GetRole`, `iam:PassRole` on `role/ORCH_STACK-*`) and `lakeformation:GetDataLakeSettings` and `lakeformation:PutDataLakeSettings`, in addition to the CloudFormation, Lambda, Step Functions, Athena, SNS and EventBridge permissions for the resources in the template. You can instead [supply both roles](#pre-created-orchestrator-roles).

For child stacks, the orchestrator either creates one shared role (`CreateSharedIAMRole` = `YES`, the default), lets each child stack create its own roles (`NO`), or uses a role you create (`SharedIAMRoleArn`). A role you create needs the [shared role policy](#pre-created-shared-role), with these values and changes:

- `STACK_PREFIX` is `s3-rollback-`.
- `BUCKET` and `NAMESPACE` cover every bucket in the input CSV, or use `*` and `b_*`.
- Add `arn:PARTITION:athena:REGION:ACCOUNT_ID:workgroup/ORCH_STACK-wg-*` to the `Athena` statement. Child stacks use the orchestrator's WorkGroups.
- Add `sts:AssumeRole` on the orchestrator's Lambda role: `arn:PARTITION:iam::ACCOUNT_ID:role/ORCH_STACK-LambdaExecutionRole-*` by default, or `ORCH_LAMBDA_ROLE_ARN` if you supply it. Child stacks make their Lake Formation grants by assuming that role, so the shared role does not need to be a Lake Formation administrator.

### Pre-created orchestrator roles

Pass **Orchestrator Lambda role ARN** (`OrchestratorLambdaRoleArn`), **Orchestrator Step Functions role ARN** (`OrchestratorStepFunctionsRoleArn`) and **Shared IAM role ARN** (`SharedIAMRoleArn`) together. The orchestrator stack and every child stack then create no IAM roles, and the orchestrator does not change the Lake Formation administrator list. Setting only one or two of the three fails template validation.

Placeholders in this section, in addition to [those above](#placeholders):

| Placeholder | Value |
|---|---|
| `ORCH_LAMBDA_ROLE_ARN` | ARN of the role you pass as `OrchestratorLambdaRoleArn` |
| `ORCH_SFN_ROLE_ARN` | ARN of the role you pass as `OrchestratorStepFunctionsRoleArn` |
| `SHARED_ROLE_ARN` | ARN of the role you pass as `SharedIAMRoleArn` |
| `RESULTS_BUCKET` | The orchestrator's results bucket |

**Lake Formation.** If Lake Formation manages permissions on the account catalog or on `s3tablescatalog/aws-s3`, make the Lambda role a data lake administrator in the Region before you deploy. Child stacks create their Glue databases and make their Lake Formation grants as this role. Each orchestrator run checks this first: if the role is not an administrator, the run fails before it creates any child stack, and the execution's failure cause names the role and the catalogs. Make the role an administrator, then start the orchestrator again. The check applies even if no bucket in the run uses S3 Metadata, because the orchestrator does not know each bucket's inventory source in advance.

Lambda role trust policy. The second statement lets child stacks assume the role for their Lake Formation grants:

<!-- policy:orch-lambda-trust -->
```json
{
  "Version": "2012-10-17",
  "Statement": [
    {"Effect": "Allow", "Principal": {"Service": "lambda.amazonaws.com"}, "Action": "sts:AssumeRole"},
    {"Effect": "Allow", "Principal": {"AWS": "arn:PARTITION:iam::ACCOUNT_ID:root"}, "Action": "sts:AssumeRole",
     "Condition": {"ArnLike": {"aws:PrincipalArn": "SHARED_ROLE_ARN"}}}
  ]
}
```

Lambda role permissions policy. Add the `FailureTopic` statement only if you set `SNSEmailList`:

<!-- policy:orch-lambda -->
```json
{
  "Version": "2012-10-17",
  "Statement": [
    {"Sid": "ReadInputCsvAndTemplate", "Effect": "Allow", "Action": ["s3:GetObject", "s3:ListBucket"], "Resource": "*"},
    {"Sid": "ChildResultsBuckets", "Effect": "Allow",
     "Action": ["s3:CreateBucket", "s3:DeleteBucket", "s3:PutBucketPublicAccessBlock", "s3:PutEncryptionConfiguration",
                "s3:GetEncryptionConfiguration", "s3:PutBucketPolicy", "s3:GetBucketPolicy", "s3:DeleteBucketPolicy",
                "s3:PutBucketTagging", "s3:GetBucketTagging"],
     "Resource": "arn:PARTITION:s3:::s3-rollback-*-athenaresultsbucket-*"},
    {"Sid": "WriteResults", "Effect": "Allow", "Action": "s3:PutObject",
     "Resource": "arn:PARTITION:s3:::RESULTS_BUCKET/results/*"},
    {"Sid": "ReadIntermediateResults", "Effect": "Allow", "Action": "s3:GetObject",
     "Resource": "arn:PARTITION:s3:::RESULTS_BUCKET/intermediate/*"},
    {"Sid": "ListResultsBucket", "Effect": "Allow", "Action": "s3:ListBucket", "Resource": "arn:PARTITION:s3:::RESULTS_BUCKET"},
    {"Sid": "ChildStacks", "Effect": "Allow",
     "Action": ["cloudformation:CreateStack", "cloudformation:DescribeStacks", "cloudformation:DeleteStack",
                "cloudformation:DescribeStackEvents"],
     "Resource": "arn:PARTITION:cloudformation:REGION:ACCOUNT_ID:stack/s3-rollback-*/*"},
    {"Sid": "ListStacks", "Effect": "Allow", "Action": "cloudformation:ListStacks", "Resource": "*"},
    {"Sid": "PassSharedRole", "Effect": "Allow", "Action": "iam:PassRole", "Resource": "SHARED_ROLE_ARN",
     "Condition": {"StringEquals": {"iam:PassedToService": "lambda.amazonaws.com"}}},
    {"Sid": "ChildLogGroups", "Effect": "Allow",
     "Action": ["logs:CreateLogGroup", "logs:TagResource", "logs:ListTagsForResource"],
     "Resource": "arn:PARTITION:logs:REGION:ACCOUNT_ID:log-group:/aws/lambda/s3-rollback/s3-rollback-*"},
    {"Sid": "DescribeLogGroups", "Effect": "Allow", "Action": "logs:DescribeLogGroups", "Resource": "*"},
    {"Sid": "OwnLogs", "Effect": "Allow", "Action": ["logs:CreateLogStream", "logs:PutLogEvents"],
     "Resource": "arn:PARTITION:logs:REGION:ACCOUNT_ID:log-group:/aws/lambda/s3-rollback/ORCH_STACK/*"},
    {"Sid": "ChildGlueDatabases", "Effect": "Allow",
     "Action": ["glue:CreateDatabase", "glue:DeleteDatabase", "glue:GetDatabase", "glue:CreateTable", "glue:DeleteTable",
                "glue:GetTable", "glue:GetTables", "glue:UpdateTable", "glue:GetUserDefinedFunctions",
                "glue:DeleteUserDefinedFunction"],
     "Resource": ["arn:PARTITION:glue:REGION:ACCOUNT_ID:catalog",
                  "arn:PARTITION:glue:REGION:ACCOUNT_ID:database/s3_rollback_*",
                  "arn:PARTITION:glue:REGION:ACCOUNT_ID:table/s3_rollback_*/*",
                  "arn:PARTITION:glue:REGION:ACCOUNT_ID:userDefinedFunction/s3_rollback_*/*"]},
    {"Sid": "GlueS3MetadataNamespaces", "Effect": "Allow", "Action": ["glue:GetCatalog", "glue:GetDatabase", "glue:GetTable"],
     "Resource": ["arn:PARTITION:glue:REGION:ACCOUNT_ID:catalog",
                  "arn:PARTITION:glue:REGION:ACCOUNT_ID:catalog/s3tablescatalog",
                  "arn:PARTITION:glue:REGION:ACCOUNT_ID:catalog/s3tablescatalog/aws-s3",
                  "arn:PARTITION:glue:REGION:ACCOUNT_ID:database/s3tablescatalog/aws-s3/b_*",
                  "arn:PARTITION:glue:REGION:ACCOUNT_ID:table/s3tablescatalog/aws-s3/b_*/*"]},
    {"Sid": "S3MetadataTables", "Effect": "Allow",
     "Action": ["s3tables:GetTableBucket", "s3tables:GetNamespace", "s3tables:GetTable", "s3tables:ListNamespaces",
                "s3tables:ListTables"],
     "Resource": ["arn:PARTITION:s3tables:REGION:ACCOUNT_ID:bucket/aws-s3",
                  "arn:PARTITION:s3tables:REGION:ACCOUNT_ID:bucket/aws-s3/table/*"]},
    {"Sid": "Athena", "Effect": "Allow",
     "Action": ["athena:CreateWorkGroup", "athena:DeleteWorkGroup", "athena:UpdateWorkGroup", "athena:GetWorkGroup",
                "athena:TagResource", "athena:CreateNamedQuery", "athena:DeleteNamedQuery", "athena:GetNamedQuery",
                "athena:StartQueryExecution", "athena:GetQueryExecution", "athena:GetQueryResults"],
     "Resource": ["arn:PARTITION:athena:REGION:ACCOUNT_ID:workgroup/ORCH_STACK-wg-*",
                  "arn:PARTITION:athena:REGION:ACCOUNT_ID:workgroup/s3_rollback_wg_*"]},
    {"Sid": "ChildFunctions", "Effect": "Allow",
     "Action": ["lambda:CreateFunction", "lambda:DeleteFunction", "lambda:GetFunction", "lambda:GetFunctionConfiguration",
                "lambda:AddPermission", "lambda:RemovePermission", "lambda:InvokeFunction", "lambda:UpdateFunctionCode",
                "lambda:UpdateFunctionConfiguration", "lambda:TagResource", "lambda:UntagResource"],
     "Resource": "arn:PARTITION:lambda:REGION:ACCOUNT_ID:function:s3-rollback-*"},
    {"Sid": "LakeFormation", "Effect": "Allow",
     "Action": ["lakeformation:GetDataLakeSettings", "lakeformation:GetDataAccess",
                "lakeformation:GrantPermissions", "lakeformation:RevokePermissions"],
     "Resource": "*"},
    {"Sid": "StartStateMachines", "Effect": "Allow", "Action": "states:StartExecution",
     "Resource": ["arn:PARTITION:states:REGION:ACCOUNT_ID:stateMachine:ORCH_STACK-orchestrator",
                  "arn:PARTITION:states:REGION:ACCOUNT_ID:stateMachine:ORCH_STACK-cleanup"]},
    {"Sid": "ManageExecutions", "Effect": "Allow", "Action": ["states:DescribeExecution", "states:StopExecution"],
     "Resource": ["arn:PARTITION:states:REGION:ACCOUNT_ID:execution:ORCH_STACK-cleanup:*",
                  "arn:PARTITION:states:REGION:ACCOUNT_ID:execution:ORCH_STACK-orchestrator:*"]},
    {"Sid": "ListExecutions", "Effect": "Allow", "Action": "states:ListExecutions",
     "Resource": "arn:PARTITION:states:REGION:ACCOUNT_ID:stateMachine:ORCH_STACK-orchestrator"},
    {"Sid": "FailureTopic", "Effect": "Allow", "Action": "sns:Subscribe",
     "Resource": "arn:PARTITION:sns:REGION:ACCOUNT_ID:ORCH_STACK-failure-notifications"}
  ]
}
```

Compared with the default `LambdaExecutionRole`, this role has no IAM role-creation actions (child stacks create no roles), passes only the shared role, and has no `lakeformation:PutDataLakeSettings`. `ReadInputCsvAndTemplate` uses `*` because the input CSV and child template are given as URLs, and the orchestrator checks each bucket in the CSV, which is only known at run time; list those buckets instead if you know them in advance.

Step Functions role trust policy:

<!-- policy:orch-sfn-trust -->
```json
{
  "Version": "2012-10-17",
  "Statement": [{"Effect": "Allow", "Principal": {"Service": "states.amazonaws.com"}, "Action": "sts:AssumeRole"}]
}
```

Step Functions role permissions policy:

<!-- policy:orch-sfn -->
```json
{
  "Version": "2012-10-17",
  "Statement": [
    {"Sid": "InvokeOrchestratorFunctions", "Effect": "Allow", "Action": "lambda:InvokeFunction",
     "Resource": ["arn:PARTITION:lambda:REGION:ACCOUNT_ID:function:ORCH_STACK-csv-parser",
                  "arn:PARTITION:lambda:REGION:ACCOUNT_ID:function:ORCH_STACK-stack-manager",
                  "arn:PARTITION:lambda:REGION:ACCOUNT_ID:function:ORCH_STACK-results-writer",
                  "arn:PARTITION:lambda:REGION:ACCOUNT_ID:function:ORCH_STACK-cleanup-stop-executions",
                  "arn:PARTITION:lambda:REGION:ACCOUNT_ID:function:ORCH_STACK-cleanup-find-stacks",
                  "arn:PARTITION:lambda:REGION:ACCOUNT_ID:function:ORCH_STACK-cleanup-delete-one",
                  "arn:PARTITION:lambda:REGION:ACCOUNT_ID:function:ORCH_STACK-cleanup-poll-one"]},
    {"Sid": "StartChildExecutions", "Effect": "Allow", "Action": "states:StartExecution",
     "Resource": ["arn:PARTITION:states:REGION:ACCOUNT_ID:stateMachine:ORCH_STACK-orchestrator",
                  "arn:PARTITION:states:REGION:ACCOUNT_ID:stateMachine:ORCH_STACK-cleanup"]},
    {"Sid": "ManageChildExecutions", "Effect": "Allow", "Action": ["states:DescribeExecution", "states:StopExecution"],
     "Resource": ["arn:PARTITION:states:REGION:ACCOUNT_ID:execution:ORCH_STACK-orchestrator/*",
                  "arn:PARTITION:states:REGION:ACCOUNT_ID:execution:ORCH_STACK-cleanup/*"]},
    {"Sid": "ListExecutions", "Effect": "Allow", "Action": "states:ListExecutions",
     "Resource": ["arn:PARTITION:states:REGION:ACCOUNT_ID:stateMachine:ORCH_STACK-orchestrator",
                  "arn:PARTITION:states:REGION:ACCOUNT_ID:stateMachine:ORCH_STACK-cleanup"]},
    {"Sid": "DistributedMapResults", "Effect": "Allow",
     "Action": ["s3:PutObject", "s3:AbortMultipartUpload", "s3:ListMultipartUploadParts"],
     "Resource": "arn:PARTITION:s3:::RESULTS_BUCKET/intermediate/*"},
    {"Sid": "VendedLogs", "Effect": "Allow",
     "Action": ["logs:CreateLogDelivery", "logs:GetLogDelivery", "logs:UpdateLogDelivery", "logs:DeleteLogDelivery",
                "logs:ListLogDeliveries", "logs:PutResourcePolicy", "logs:DescribeResourcePolicies", "logs:DescribeLogGroups"],
     "Resource": "*"}
  ]
}
```

The Step Functions guide requires `*` for [log delivery to CloudWatch Logs](https://docs.aws.amazon.com/step-functions/latest/dg/cw-logs.html).

**Deploying principal.** With supplied roles, the deploying principal needs no IAM role creation and no `lakeformation:PutDataLakeSettings`. It needs the CloudFormation, Lambda, Step Functions, Athena, CloudWatch Logs, SNS and EventBridge permissions for the resources in the template, plus:

<!-- policy:orch-deployer-supplied-roles -->
```json
[
  {"Sid": "PassOrchestratorLambdaRole", "Effect": "Allow", "Action": "iam:PassRole", "Resource": "ORCH_LAMBDA_ROLE_ARN",
   "Condition": {"StringEquals": {"iam:PassedToService": "lambda.amazonaws.com"}}},
  {"Sid": "PassOrchestratorStepFunctionsRole", "Effect": "Allow", "Action": "iam:PassRole", "Resource": "ORCH_SFN_ROLE_ARN",
   "Condition": {"StringEquals": {"iam:PassedToService": "states.amazonaws.com"}}}
]
```

Switch between created and supplied roles only when the orchestrator has no child stacks. Existing child stacks keep the Lambda role they were created with for their Lake Formation revokes.

## Organizations that restrict IAM role names

If a service control policy (SCP) or permissions boundary rule blocks the role names CloudFormation generates, create the roles yourself, named and placed to meet your rules, and pass their ARNs. No template then creates an IAM role.

| Template | Parameters to set |
|---|---|
| `s3-rollback.yaml` | `SharedIAMRoleArn` ([policy](#pre-created-shared-role)) |
| `s3-rollback-glue-metadata.yaml` | `SharedIAMRoleArn` and `GlueJobRoleArn` ([policy](#large-scale-template-s3-rollback-glue-metadatayaml)) |
| `s3-rollback-orchestrator.yaml` | `SharedIAMRoleArn`, `OrchestratorLambdaRoleArn` and `OrchestratorStepFunctionsRoleArn` ([policy](#pre-created-orchestrator-roles)) |

Stack names must still start with the prefix your role policies are scoped to: `STACK_PREFIX` for the rollback templates, and `s3-rollback-` for the orchestrator's child stacks, which the orchestrator names itself.

## Large-scale template (`s3-rollback-glue-metadata.yaml`)

This template runs an AWS Glue job, so pre-created roles come as a pair: pass both **Shared IAM role ARN** (`SharedIAMRoleArn`) and **Glue job role ARN** (`GlueJobRoleArn`), or neither. It also accepts `LFAdminRoleArn`, as [above](#lake-formation).

The shared role uses the [shared role policy](#pre-created-shared-role) without the `GlueS3MetadataNamespace` statement (this template's Athena queries read only the stack's own database), plus:

<!-- policy:glue-shared-additions -->
```json
[
  {"Sid": "StartPitJob", "Effect": "Allow", "Action": "glue:StartJobRun",
   "Resource": "arn:PARTITION:glue:REGION:ACCOUNT_ID:job/STACK_PREFIX*-pit-table"},
  {"Sid": "PassGlueJobRole", "Effect": "Allow", "Action": "iam:PassRole",
   "Resource": "arn:PARTITION:iam::ACCOUNT_ID:role/GLUE_JOB_ROLE_NAME",
   "Condition": {"StringEquals": {"iam:PassedToService": "glue.amazonaws.com"}}}
]
```

The Glue job role trusts `glue.amazonaws.com` and needs:

<!-- policy:glue-job -->
```json
{
  "Version": "2012-10-17",
  "Statement": [
    {"Sid": "GlueLogs", "Effect": "Allow", "Action": ["logs:CreateLogGroup", "logs:CreateLogStream", "logs:PutLogEvents"],
     "Resource": "arn:PARTITION:logs:REGION:ACCOUNT_ID:log-group:/aws-glue/*"},
    {"Sid": "GlueMetrics", "Effect": "Allow", "Action": "cloudwatch:PutMetricData", "Resource": "*",
     "Condition": {"StringEquals": {"cloudwatch:namespace": "Glue"}}},
    {"Sid": "ResultsBucket", "Effect": "Allow", "Action": ["s3:ListBucket", "s3:GetBucketLocation"],
     "Resource": "arn:PARTITION:s3:::STACK_PREFIX*-athenaresultsbucket-*"},
    {"Sid": "ResultsObjects", "Effect": "Allow", "Action": ["s3:GetObject", "s3:PutObject", "s3:DeleteObject"],
     "Resource": "arn:PARTITION:s3:::STACK_PREFIX*-athenaresultsbucket-*/*"},
    {"Sid": "S3MetadataTables", "Effect": "Allow",
     "Action": ["s3tables:GetTableBucket", "s3tables:GetNamespace", "s3tables:ListNamespaces", "s3tables:GetTable",
                "s3tables:ListTables", "s3tables:GetTableData", "s3tables:GetTableMetadataLocation"],
     "Resource": ["arn:PARTITION:s3tables:REGION:ACCOUNT_ID:bucket/aws-s3",
                  "arn:PARTITION:s3tables:REGION:ACCOUNT_ID:bucket/aws-s3/table/*"]},
    {"Sid": "S3TablesDataAccessPoint", "Effect": "Allow", "Action": ["s3:GetObject", "s3:ListBucket", "s3:GetBucketLocation"],
     "Resource": "*", "Condition": {"StringEquals": {"s3:DataAccessPointAccount": "ACCOUNT_ID"}}},
    {"Sid": "LakeFormation", "Effect": "Allow", "Action": "lakeformation:GetDataAccess", "Resource": "*"}
  ]
}
```

S3 Tables serves table data through an access point that AWS manages, so its ARN is not known in advance; the condition limits the statement to access points in your account. The deploying principal also needs `iam:PassRole` on the Glue job role (passed to `glue.amazonaws.com`); when the stack creates its own roles, `iam:AttachRolePolicy` and `iam:DetachRolePolicy` on `role/STACK_PREFIX*`, because the stack's Glue job role uses the AWS managed policy `AWSGlueServiceRole`; `glue:CreateJob`, `glue:DeleteJob` and `glue:GetJob` on `job/STACK_PREFIX*`, `events:PutRule`, `events:PutTargets`, `events:RemoveTargets`, `events:DeleteRule` and `events:DescribeRule` on `rule/STACK_PREFIX*`, and `lambda:AddPermission` and `lambda:RemovePermission` on `function:STACK_PREFIX*`.

## KMS keys

**Copy using KMS key** (`KMSKey`). Copies are encrypted with this key. The tool does not change the key policy. The role that copies (the shared role, or the stack's `S3BatchOpsExecutorRole` and `S3BatchOpsCopyFunctionRole`) needs `kms:GenerateDataKey` and `kms:Decrypt` on the key, granted in the key policy or in the role. If source objects are encrypted with a customer managed key, the same roles need `kms:Decrypt` on that key.

**S3 Metadata tables KMS key** (`ExistingKmsKeyIdOrArn`). The stack adds a statement to the key policy so that its roles can read the encrypted metadata tables, and removes it when the stack is deleted. With a pre-created shared role, either:

- give the shared role `kms:DescribeKey`, `kms:GetKeyPolicy` and `kms:PutKeyPolicy` on that key, and the stack manages the statement; or
- add the statement to the key policy yourself, and leave those three actions out. The stack checks that the shared role can use the key and changes nothing. For the large-scale template, add the Glue job role to the statement too.

<!-- policy:kms-key-policy-statement -->
```json
{"Sid": "AllowS3RollbackSharedRole", "Effect": "Allow",
 "Principal": {"AWS": "arn:PARTITION:iam::ACCOUNT_ID:role/ROLE_NAME"},
 "Action": ["kms:Decrypt", "kms:GenerateDataKey", "kms:DescribeKey"], "Resource": "*"}
```
