#!/usr/bin/env bash
# Title Protocol — Launch a one-shot SP1 prover EC2.
#
# Spins up a c5.12xlarge (48 vCPU / 96 GiB) in the SAME subnet + SG as the
# TEE node so SSH between the two works without extra firewall rules.
# Returns the instance id and public IP.
#
# Usage:
#   bash deploy/aws/scripts/prover-launch.sh
#
# Output: writes INSTANCE_ID and PUBLIC_IP (one per line) to stdout.
# Cost: ~$2/hr in Tokyo. Terminate with prover-terminate.sh when done.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
TEE_NAME="${TEE_NAME:-title-protocol-node}"
PROVER_NAME="${PROVER_NAME:-title-protocol-prover}"
INSTANCE_TYPE="${INSTANCE_TYPE:-c5.12xlarge}"

read -r SUBNET_ID SG_ID KEY_NAME <<<"$(aws ec2 describe-instances \
  --filters "Name=tag:Name,Values=${TEE_NAME}" "Name=instance-state-name,Values=running" \
  --query 'Reservations[0].Instances[0].[SubnetId,SecurityGroups[0].GroupId,KeyName]' \
  --output text)"

if [[ -z "${SUBNET_ID}" || "${SUBNET_ID}" == "None" ]]; then
  echo "ERROR: could not discover subnet/SG from TEE node tagged '${TEE_NAME}'." >&2
  echo "       Run 'terraform apply' in deploy/aws/terraform first." >&2
  exit 1
fi

echo "==> Launching ${INSTANCE_TYPE} prover in subnet ${SUBNET_ID} (key ${KEY_NAME})" >&2

INSTANCE_ID=$(aws ec2 run-instances \
  --image-id resolve:ssm:/aws/service/ami-amazon-linux-latest/al2023-ami-kernel-default-x86_64 \
  --instance-type "${INSTANCE_TYPE}" \
  --key-name "${KEY_NAME}" \
  --security-group-ids "${SG_ID}" \
  --subnet-id "${SUBNET_ID}" \
  --associate-public-ip-address \
  --tag-specifications "ResourceType=instance,Tags=[{Key=Name,Value=${PROVER_NAME}}]" \
  --block-device-mappings '[{"DeviceName":"/dev/xvda","Ebs":{"VolumeSize":50}}]' \
  --query 'Instances[0].InstanceId' --output text)

echo "==> Waiting for instance ${INSTANCE_ID} to be running" >&2
aws ec2 wait instance-running --instance-ids "${INSTANCE_ID}"

PUBLIC_IP=$(aws ec2 describe-instances --instance-ids "${INSTANCE_ID}" \
  --query 'Reservations[0].Instances[0].PublicIpAddress' --output text)

echo "==> Waiting for SSH on ${PUBLIC_IP}" >&2
KEY_PATH="${REPO_ROOT}/deploy/aws/keys/${KEY_NAME}.pem"
for _ in $(seq 1 30); do
  if ssh -i "${KEY_PATH}" -o StrictHostKeyChecking=no -o ConnectTimeout=5 \
       ec2-user@"${PUBLIC_IP}" 'true' >/dev/null 2>&1; then
    break
  fi
  sleep 5
done

echo "${INSTANCE_ID}"
echo "${PUBLIC_IP}"
