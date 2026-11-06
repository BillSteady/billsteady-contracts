#!/usr/bin/env bash
# Second half of the real-time 24h-delay test (GitHub issue #4). The request
# half already ran — see pending-24h-release-test.md for the identities,
# contract ID, and request transaction hash. This script runs the two
# remaining checks once available_at has passed:
#   1. claim_early_release succeeds and releases the jar
#   2. (run this FIRST, before #1, if you want proof of the negative case)
#      a claim attempt before available_at is rejected by the real network
#
# Usage: ./scripts/claim-early-release-demo.sh
set -euo pipefail
cd "$(dirname "$0")/.."

JAR=CDI3CC2JGI4I4IMZ4P4GCEYF2A4UCBM6WE3ZHVCESNU2RBMIZ42PX5AO

echo "==> claim_early_release (jar_id 0)"
stellar contract invoke --id "$JAR" --source-account jar-delay-test-owner --network testnet \
  -- claim_early_release --jar_id 0

echo "==> reading back jar state to confirm it actually released"
stellar contract invoke --id "$JAR" --source-account jar-delay-test-owner --network testnet \
  --send=no -- get_jar --jar_id 0
