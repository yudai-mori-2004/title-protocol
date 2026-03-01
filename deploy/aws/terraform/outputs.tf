# Title Protocol Devnet — Terraform 出力値

output "nodes" {
  description = "全ノードの情報"
  value = [
    for i in range(var.node_count) : {
      index       = i
      instance_id = aws_instance.node[i].id
      public_ip   = aws_eip.node[i].public_ip
      gateway_url = "http://${aws_eip.node[i].public_ip}:3000"
      ssh_command = "ssh -i ${var.key_file} ec2-user@${aws_eip.node[i].public_ip}"
    }
  ]
}

output "s3_bucket_name" {
  description = "Temporary Storage S3バケット名（全ノード共有）"
  value       = aws_s3_bucket.uploads.bucket
}

output "s3_bucket_endpoint" {
  description = "S3バケットのリージョナルエンドポイント"
  value       = "https://s3.${var.aws_region}.amazonaws.com"
}

output "s3_access_key_id" {
  description = "S3アクセスキーID（.env の S3_ACCESS_KEY に設定）"
  value       = aws_iam_access_key.s3_user_key.id
}

output "s3_secret_access_key" {
  description = "S3シークレットアクセスキー（.env の S3_SECRET_KEY に設定）"
  value       = aws_iam_access_key.s3_user_key.secret
  sensitive   = true
}

output "security_group_id" {
  description = "セキュリティグループID"
  value       = aws_security_group.main.id
}
