# Title Protocol Devnet — Terraform 変数定義

variable "aws_region" {
  description = "AWSリージョン"
  type        = string
  default     = "ap-northeast-1"
}

variable "instance_type" {
  description = "EC2インスタンスタイプ（Nitro Enclave対応が必要）"
  type        = string
  default     = "c5.xlarge"
}

variable "key_name" {
  description = "SSH接続用のEC2キーペア名"
  type        = string
  default     = "title-protocol-devnet"
}

variable "key_file" {
  description = "SSH秘密鍵ファイルのパス（deploy/keys/ に配置）"
  type        = string
  default     = "../keys/title-protocol-devnet.pem"
}

variable "allowed_ssh_cidrs" {
  description = "SSH許可するCIDRブロック"
  type        = list(string)
  default     = ["0.0.0.0/0"]
}

variable "project_name" {
  description = "プロジェクト名（リソース命名用）"
  type        = string
  default     = "title-protocol"
}

variable "s3_bucket_name" {
  description = "Temporary Storage用S3バケット名"
  type        = string
  default     = "title-uploads-devnet"
}

variable "volume_size" {
  description = "EBSボリュームサイズ（GB）"
  type        = number
  default     = 50
}

variable "enclave_cpu_count" {
  description = "Enclaveに割り当てるvCPU数"
  type        = number
  default     = 2
}

variable "enclave_memory_mib" {
  description = "Enclaveに割り当てるメモリ（MiB）"
  type        = number
  default     = 512
}
