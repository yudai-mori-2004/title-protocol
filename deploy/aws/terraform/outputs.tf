# Title Protocol Devnet — Terraform 出力値

output "instance_public_ip" {
  description = "EC2インスタンスのパブリックIP"
  value       = aws_instance.main.public_ip
}

output "instance_id" {
  description = "EC2インスタンスID"
  value       = aws_instance.main.id
}

output "s3_bucket_name" {
  description = "Temporary Storage S3バケット名"
  value       = aws_s3_bucket.uploads.bucket
}

output "s3_bucket_endpoint" {
  description = "S3バケットのリージョナルエンドポイント"
  value       = "https://s3.${var.aws_region}.amazonaws.com"
}

output "gateway_url" {
  description = "Gateway APIエンドポイント"
  value       = "http://${aws_instance.main.public_ip}:3000"
}

output "ssh_command" {
  description = "SSH接続コマンド"
  value       = "ssh -i ${var.key_file} ec2-user@${aws_instance.main.public_ip}"
}

output "security_group_id" {
  description = "セキュリティグループID"
  value       = aws_security_group.main.id
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
