#!/bin/sh
# testaruda exec loop — DDL test-runner standard (see dulce-de-leche repo
# ticket; shape matches tambor tools/gates/testaruda-exec.sh, tambor-j44).
#
# exec loop: select → run → ingest → calibrate.
# Exit 0 = selected tests ran and passed; exit 20 = no tests affected;
# exit 10 = store uncalibrated. Any other code blocks.
#
# Calibration veto (tambor-cpd policy, adopted as the DDL standard): with
# 0 actual failures, NOT CALIBRATED is a vacuous pass (recall@k 0/0) and is
# said out loud, never silent. With actual failures > 0 it blocks.
# runs (same latent bug fixed in ci.yml, tambor-kpx).
#
# Tolerated codes: 0 = ran and passed, 20 = nothing affected, 10 =
# store uncalibrated (first run after init/reset — calibration is
# non-blocking, tambor-cpd).
#
# Calibration ruling (tambor-cpd, 2026-10-14): the gate is non-blocking
# for the vacuous case only. With 0 actual failures, recall@k is
# degenerate (0/0) — a clean run gives no ranking evidence either way,
# so NOT CALIBRATED there is a VACUOUS PASS and is said out loud, never
# silent. With actual failures > 0, a low-recall ranking normally
# co-occurs with genuinely failing tests (exec propagates the runner's
# nonzero exit before calibrate could matter); if NOT CALIBRATED ever
# shows with an otherwise-tolerated exit, it blocks. Unparseable
# calibration output fails loud. testaruda upstream (PR #85) makes the
# engine print the vacuous pass itself; this wrapper keeps the guarantee
# for the pre-release engine too.

set -eu

code=0
tmp=$(mktemp /tmp/testaruda-exec.XXXXXX.log)
trap 'rm -f "$tmp"' EXIT INT TERM

if testaruda exec >"$tmp" 2>&1; then
  code=0
else
  code=$?
fi
cat "$tmp"

if [ "$code" -eq 0 ] || [ "$code" -eq 10 ] || [ "$code" -eq 20 ]; then
  if grep -q "NOT CALIBRATED" "$tmp"; then
    if grep -q "Actual failures: 0" "$tmp"; then
      echo "testaruda: calibration gate vacuous pass — 0 actual failures, recall@k degenerate 0/0 (nothing to rank; non-blocking per tambor-cpd)"
    else
      echo "testaruda: NOT CALIBRATED with actual failures > 0 or unparseable output — blocking per tambor-cpd" >&2
      exit 1
    fi
  fi
  exit 0
fi
echo "testaruda exec failed with exit code $code" >&2
exit "$code"
