# Title Protocol — Terraform outputs
#
# Public IP changes on every stop/start because we don't allocate an
# Elastic IP. Always re-read these outputs after restarting the instance.

output "instance_id" {
  description = "EC2 instance ID"
  value       = aws_instance.node.id
}

output "public_ip" {
  description = "Current public IP (changes on stop/start)"
  value       = aws_instance.node.public_ip
}

output "ssh_command" {
  description = "Ready-to-paste SSH command"
  value       = "ssh -i deploy/aws/keys/${var.key_name}.pem ec2-user@${aws_instance.node.public_ip}"
}

output "gateway_url" {
  description = "Public gateway endpoint (HTTP)"
  value       = "http://${aws_instance.node.public_ip}:3000"
}

output "key_path" {
  description = "Where the freshly generated SSH private key was written"
  value       = local_sensitive_file.ssh_private_key.filename
}
