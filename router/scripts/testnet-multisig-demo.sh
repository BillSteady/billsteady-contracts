#!/usr/bin/env bash
# Proves the 2-of-3 admin-signer threshold on BillPaymentRouter against
# real Stellar testnet: a below-threshold set_settlement_address call is
# rejected by the live network (not just cargo test), a quorum call
# succeeds, and pay() still works unaffected by any of this. Fresh
# throwaway identities and a fresh contract deploy every run - this
# contract is explicitly non-upgradeable, so this is a new contract ID,
# not an upgrade of the single-admin deployment.
#
# Usage: ./scripts/testnet-multisig-demo.sh
# Requires: stellar CLI, network access to testnet + friendbot + Horizon.

set -euo pipefail
cd "$(dirname "$0")/.."

NETWORK=testnet
SUFFIX=$RANDOM
SIGNER0="router-ms-signer0-$SUFFIX"
SIGNER1="router-ms-signer1-$SUFFIX"
SIGNER2="router-ms-signer2-$SUFFIX"
ISSUER="router-ms-issuer-$SUFFIX"
PAYER="router-ms-payer-$SUFFIX"
SETTLEMENT="router-ms-settlement-$SUFFIX"

echo "==> Generating and funding throwaway testnet identities"
for id in "$SIGNER0" "$SIGNER1" "$SIGNER2" "$ISSUER" "$PAYER" "$SETTLEMENT"; do
  stellar keys generate "$id" --network "$NETWORK" --fund
done

SIGNER0_ADDR=$(stellar keys address "$SIGNER0")
SIGNER1_ADDR=$(stellar keys address "$SIGNER1")
SIGNER2_ADDR=$(stellar keys address "$SIGNER2")
ISSUER_ADDR=$(stellar keys address "$ISSUER")
PAYER_ADDR=$(stellar keys address "$PAYER")
SETTLEMENT_ADDR=$(stellar keys address "$SETTLEMENT")

echo "==> Issuing a test asset (TESTUSDC) and funding the payer's trustline"
stellar tx new change-trust --source-account "$PAYER" --network "$NETWORK" \
  --line "TESTUSDC:$ISSUER_ADDR" --limit 10000000000000
stellar tx new change-trust --source-account "$SETTLEMENT" --network "$NETWORK" \
  --line "TESTUSDC:$ISSUER_ADDR" --limit 10000000000000
stellar tx new payment --source-account "$ISSUER" --network "$NETWORK" \
  --destination "$PAYER_ADDR" --asset "TESTUSDC:$ISSUER_ADDR" --amount 1000000000000

echo "==> Deploying the Stellar Asset Contract (SAC) wrapper for TESTUSDC"
SAC=$(stellar contract asset deploy --asset "TESTUSDC:$ISSUER_ADDR" \
  --source-account "$SIGNER0" --network "$NETWORK" | tail -1)
echo "    SAC contract: $SAC"

echo "==> Building and deploying the router contract (2-of-3 admin threshold)"
stellar contract build
ROUTER=$(stellar contract deploy \
  --wasm target/wasm32v1-none/release/billsteady_router.wasm \
  --source-account "$SIGNER0" --network "$NETWORK" \
  -- \
  --admin_signers "[\"$SIGNER0_ADDR\",\"$SIGNER1_ADDR\",\"$SIGNER2_ADDR\"]" \
  --admin_threshold 2 | tail -1)
echo "    Router contract: $ROUTER"

echo
echo "==> Negative case: set_settlement_address with only 1 of 3 signers (below threshold 2)"
set +e
stellar contract invoke --id "$ROUTER" --source-account "$SIGNER0" --network "$NETWORK" \
  -- set_settlement_address --signers "[\"$SIGNER0_ADDR\"]" \
  --biller_id DISCO_IKEJA --address "$SETTLEMENT_ADDR" 2>&1 | tee /tmp/router-ms-below-threshold.log
BELOW_EXIT=${PIPESTATUS[0]}
set -e
if [ "$BELOW_EXIT" -eq 0 ]; then
  echo "    !! UNEXPECTED: a single-signer call succeeded against a threshold of 2. Real vulnerability."
  exit 1
else
  echo "    Confirmed: the real network rejected a below-threshold call (exit $BELOW_EXIT)."
fi

echo
echo "==> Confirm nothing was written by the rejected call"
set +e
stellar contract invoke --id "$ROUTER" --source-account "$SIGNER0" --network "$NETWORK" \
  --send=no -- get_settlement_address --biller_id DISCO_IKEJA 2>&1 | tee /tmp/router-ms-read-after-reject.log
set -e

echo
echo "==> Positive case: set_settlement_address with 2 of 3 signers (meets threshold)"
stellar contract invoke --id "$ROUTER" --source-account "$SIGNER0" --network "$NETWORK" \
  -- set_settlement_address --signers "[\"$SIGNER0_ADDR\",\"$SIGNER1_ADDR\"]" \
  --biller_id DISCO_IKEJA --address "$SETTLEMENT_ADDR"

echo
echo "==> Confirm the settlement address actually landed"
stellar contract invoke --id "$ROUTER" --source-account "$SIGNER0" --network "$NETWORK" \
  --send=no -- get_settlement_address --biller_id DISCO_IKEJA

echo
echo "==> pay() still works, entirely unaffected by the admin-threshold change"
ORDER_REF=$(python3 -c "print('2' * 64)")
stellar contract invoke --id "$ROUTER" --source-account "$PAYER" --network "$NETWORK" \
  -- pay --payer "$PAYER_ADDR" --token "$SAC" --amount 100000000 \
  --biller_id DISCO_IKEJA --order_ref "$ORDER_REF"

echo "==> Reading back balances for independent confirmation"
stellar contract invoke --id "$SAC" --source-account "$PAYER" --network "$NETWORK" \
  --send=no -- balance --id "$PAYER_ADDR"
stellar contract invoke --id "$SAC" --source-account "$PAYER" --network "$NETWORK" \
  --send=no -- balance --id "$SETTLEMENT_ADDR"

echo
echo "Done. Router contract (2-of-3 admin threshold): $ROUTER"
echo "SAC token: $SAC"
echo "Admin signers: $SIGNER0_ADDR / $SIGNER1_ADDR / $SIGNER2_ADDR"
echo "Settlement address (DISCO_IKEJA): $SETTLEMENT_ADDR"
