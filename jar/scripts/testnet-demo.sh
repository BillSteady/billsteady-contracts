#!/usr/bin/env bash
# End-to-end demo of the jar contract on Stellar testnet: issues a test
# TESTUSDC asset, wraps it in a Stellar Asset Contract (SAC), deploys the
# jar contract, and runs create_jar -> deposit -> pay_out -> withdraw
# against the real deployed contract, printing every transaction hash.
#
# This is a demo/verification script, not a deploy pipeline for real money.
# It generates fresh throwaway testnet identities every run (funded for
# free by friendbot) rather than reusing any real admin key.
#
# Usage: ./scripts/testnet-demo.sh
# Requires: stellar CLI, network access to testnet + friendbot + Horizon.

set -euo pipefail
cd "$(dirname "$0")/.."

NETWORK=testnet
SUFFIX=$RANDOM
ADMIN="demo-admin-$SUFFIX"
SIGNER2="demo-signer2-$SUFFIX"
SIGNER3="demo-signer3-$SUFFIX"
ISSUER="demo-issuer-$SUFFIX"
OWNER="demo-owner-$SUFFIX"
PAYEE="demo-payee-$SUFFIX"

echo "==> Generating and funding throwaway testnet identities"
for id in "$ADMIN" "$SIGNER2" "$SIGNER3" "$ISSUER" "$OWNER" "$PAYEE"; do
  stellar keys generate "$id" --network "$NETWORK" --fund
done

ADMIN_ADDR=$(stellar keys address "$ADMIN")
SIGNER2_ADDR=$(stellar keys address "$SIGNER2")
SIGNER3_ADDR=$(stellar keys address "$SIGNER3")
ISSUER_ADDR=$(stellar keys address "$ISSUER")
OWNER_ADDR=$(stellar keys address "$OWNER")
PAYEE_ADDR=$(stellar keys address "$PAYEE")

echo "==> Issuing a test asset (TESTUSDC) and funding owner + payee trustlines"
stellar tx new change-trust --source-account "$OWNER" --network "$NETWORK" \
  --line "TESTUSDC:$ISSUER_ADDR" --limit 10000000000000
stellar tx new change-trust --source-account "$PAYEE" --network "$NETWORK" \
  --line "TESTUSDC:$ISSUER_ADDR" --limit 10000000000000
stellar tx new payment --source-account "$ISSUER" --network "$NETWORK" \
  --destination "$OWNER_ADDR" --asset "TESTUSDC:$ISSUER_ADDR" --amount 1000000000000

echo "==> Deploying the Stellar Asset Contract (SAC) wrapper for TESTUSDC"
SAC=$(stellar contract asset deploy --asset "TESTUSDC:$ISSUER_ADDR" \
  --source-account "$ADMIN" --network "$NETWORK" | tail -1)
echo "    SAC contract: $SAC"

echo "==> Building and deploying the jar contract"
stellar contract build
JAR=$(stellar contract deploy \
  --wasm target/wasm32v1-none/release/billsteady_jar.wasm \
  --source-account "$ADMIN" --network "$NETWORK" \
  -- \
  --oper_admin "$ADMIN_ADDR" \
  --upgrade_signers '["'"$ADMIN_ADDR"'","'"$SIGNER2_ADDR"'","'"$SIGNER3_ADDR"'"]' \
  --upgrade_threshold 2 \
  --allowed_tokens '["'"$SAC"'"]' \
  --payees '["'"$PAYEE_ADDR"'"]' | tail -1)
echo "    Jar contract: $JAR"

UNLOCK_TIME=$(( $(date +%s) + 2592000 )) # 30 days out, informational only (locked=false below)

echo "==> create_jar (locked=false, so withdraw is immediately eligible)"
stellar contract invoke --id "$JAR" --source-account "$OWNER" --network "$NETWORK" \
  -- create_jar --owner "$OWNER_ADDR" --token "$SAC" --target 500000000 \
  --unlock_time "$UNLOCK_TIME" --locked false

echo "==> deposit 20 TESTUSDC"
stellar contract invoke --id "$JAR" --source-account "$OWNER" --network "$NETWORK" \
  -- deposit --jar_id 0 --from "$OWNER_ADDR" --amount 200000000

echo "==> pay_out 5 TESTUSDC to the allowlisted payee"
ORDER_REF=$(python3 -c "print('DEMO-ORDER'.encode().hex())")
stellar contract invoke --id "$JAR" --source-account "$OWNER" --network "$NETWORK" \
  -- pay_out --jar_id 0 --payee "$PAYEE_ADDR" --amount 50000000 --order_ref "$ORDER_REF"

echo "==> withdraw 3 TESTUSDC back to the owner (jar is unlocked: locked=false)"
stellar contract invoke --id "$JAR" --source-account "$OWNER" --network "$NETWORK" \
  -- withdraw --jar_id 0 --amount 30000000

echo "==> Reading back jar state for independent confirmation"
stellar contract invoke --id "$JAR" --source-account "$OWNER" --network "$NETWORK" \
  --send=no -- get_jar --jar_id 0

echo
echo "Done. Jar contract: $JAR"
echo "SAC token: $SAC"
echo "Note: this exercises the 'jar not locked' withdraw path. The 24h"
echo "early-release-delay path (request_early_release -> claim_early_release)"
echo "is covered by cargo unit tests with simulated ledger time, not by this"
echo "script, since a real 24h wait isn't practical here."
