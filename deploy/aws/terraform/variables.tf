# Title Protocol — Terraform variables

variable "aws_region" {
  description = "AWS region"
  type        = string
  default     = "ap-northeast-1"
}

variable "project_name" {
  description = "Project name (used as a prefix for AWS resource names)"
  type        = string
  default     = "title-protocol"
}

variable "instance_type" {
  description = <<-EOT
    EC2 instance type. Must support Nitro Enclaves.
    c5.xlarge is the practical minimum — c5.large (2 vCPU) leaves nothing
    for the host once the Enclave grabs its hyperthread pair.
  EOT
  type        = string
  default     = "c5.xlarge"
}

variable "key_name" {
  description = "Name of the AWS key pair (terraform generates the matching ed25519 key in keys/)"
  type        = string
  default     = "title-protocol-devnet"
}

variable "allowed_ssh_cidrs" {
  description = "CIDR blocks allowed to SSH in. Narrow this in production."
  type        = list(string)
  default     = ["0.0.0.0/0"]
}

variable "allowed_gateway_cidrs" {
  description = "CIDR blocks allowed to reach the Gateway HTTP port."
  type        = list(string)
  default     = ["0.0.0.0/0"]
}

variable "volume_size" {
  description = "Root EBS volume size in GB. 50 GB fits Docker images + working space comfortably."
  type        = number
  default     = 50
}

variable "enclave_cpu_count" {
  description = "vCPUs allocated to the Nitro Enclave (must be even — Enclave grabs hyperthread pairs)."
  type        = number
  default     = 2
}

variable "enclave_memory_mib" {
  description = "Memory allocated to the Nitro Enclave (MiB). v0.1.2 is comfortable at 2048; raise for bigger workloads."
  type        = number
  default     = 2048
}
