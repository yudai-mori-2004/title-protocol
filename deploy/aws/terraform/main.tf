# Title Protocol — AWS infrastructure (v0.1.2)
#
# Single Nitro-Enclaves-capable EC2 + minimal security group + auto-generated
# SSH key. `terraform apply` provisions the full TEE node; `terraform destroy`
# removes it. The public IP is reassigned on each stop/start — re-read
# `terraform output -raw public_ip` after restarting the instance.

terraform {
  required_version = ">= 1.5"
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.0"
    }
    tls = {
      source  = "hashicorp/tls"
      version = "~> 4.0"
    }
    local = {
      source  = "hashicorp/local"
      version = "~> 2.0"
    }
  }
}

provider "aws" {
  region = var.aws_region
}

# ---------------------------------------------------------------------------
# AMI — Amazon Linux 2023, x86_64 (Nitro Enclaves require x86_64 host)
# ---------------------------------------------------------------------------

data "aws_ami" "al2023" {
  most_recent = true
  owners      = ["amazon"]

  filter {
    name   = "name"
    values = ["al2023-ami-*-x86_64"]
  }

  filter {
    name   = "architecture"
    values = ["x86_64"]
  }

  filter {
    name   = "virtualization-type"
    values = ["hvm"]
  }
}

# ---------------------------------------------------------------------------
# SSH key — ed25519, generated locally on every `terraform apply`
# ---------------------------------------------------------------------------
#
# Cloning users get a working keypair in `deploy/aws/keys/` after one
# `terraform apply`; no manual `ssh-keygen` required. The private key is
# written with 0600 permissions and is `.gitignore`d.

resource "tls_private_key" "ssh" {
  algorithm = "ED25519"
}

resource "aws_key_pair" "node" {
  key_name   = var.key_name
  public_key = tls_private_key.ssh.public_key_openssh

  tags = {
    Project = var.project_name
  }
}

resource "local_sensitive_file" "ssh_private_key" {
  content         = tls_private_key.ssh.private_key_openssh
  filename        = "${path.module}/../keys/${var.key_name}.pem"
  file_permission = "0600"
}

# ---------------------------------------------------------------------------
# Security group — SSH + Gateway only
# ---------------------------------------------------------------------------

resource "aws_security_group" "node" {
  name        = "${var.project_name}-node"
  description = "Title Protocol TEE node - SSH + Gateway"

  ingress {
    from_port   = 22
    to_port     = 22
    protocol    = "tcp"
    cidr_blocks = var.allowed_ssh_cidrs
    description = "SSH"
  }

  ingress {
    from_port   = 3000
    to_port     = 3000
    protocol    = "tcp"
    cidr_blocks = var.allowed_gateway_cidrs
    description = "Gateway HTTP API"
  }

  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
    description = "All outbound (TEE fetches content, queries Solana RPC)"
  }

  tags = {
    Project = var.project_name
  }
}

# ---------------------------------------------------------------------------
# EC2 instance — Nitro Enclaves enabled, public IP auto-assigned
# ---------------------------------------------------------------------------

resource "aws_instance" "node" {
  ami                    = data.aws_ami.al2023.id
  instance_type          = var.instance_type
  key_name               = aws_key_pair.node.key_name
  vpc_security_group_ids = [aws_security_group.node.id]

  enclave_options {
    enabled = true
  }

  root_block_device {
    volume_size = var.volume_size
    volume_type = "gp3"
  }

  # First-boot provisioning (Docker, nitro-cli, hugepages) — see user-data.sh.
  # Edits to user-data.sh don't rebuild the instance; rerun by re-execing
  # the script over SSH if needed.
  user_data = templatefile("${path.module}/user-data.sh", {
    enclave_memory_mib = var.enclave_memory_mib
    enclave_cpu_count  = var.enclave_cpu_count
  })

  tags = {
    Name    = "${var.project_name}-node"
    Project = var.project_name
  }
}
