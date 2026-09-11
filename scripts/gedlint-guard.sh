# Fail the step when the action's own script dies before it finishes.
#
# Sourced by action.yml. Two bash behaviours conspire to report a crash as a
# success (issue #60):
#
#   - An EXIT trap whose last command succeeds rewrites the shell's exit
#     status to that command's status, so `trap 'rm -rf "$D"' EXIT` quietly
#     turns every failure into 0.
#   - bash 3.2, still the system bash on macOS runners, enters the EXIT trap
#     with $? == 0 after a `set -u` fatal error, so reading $? in the trap is
#     not enough to notice on its own.
#
# Together those are how an "unbound variable" crash produced no report, wrote
# no outputs, and was still recorded as outcome=success. So do not infer
# success from a status: track completion explicitly. Anything that leaves
# before gedlint_guard_done fails, and the trap always ends by exiting a
# status it chose itself.
#
# The caller sets REPORTS to its temp directory once it has one; the guard
# removes it on the way out, whichever way that is.

GEDLINT_GUARD_DONE=0
REPORTS=""

# Call on the last line the script is meant to reach, before its real exit.
gedlint_guard_done() { GEDLINT_GUARD_DONE=1; }

gedlint_guard_exit() {
  STATUS=$?
  [ -z "$REPORTS" ] || rm -rf "$REPORTS" || true
  # A non-zero status already fails the step and is either gedlint's own
  # contract (1 warnings, 2 errors) or a deliberate validation exit, so it
  # passes through untouched. Only "claims success but never finished" is a
  # lie that has to be corrected.
  if [ "$GEDLINT_GUARD_DONE" -ne 1 ] && [ "$STATUS" -eq 0 ]; then
    echo "::error::gedlint action: the script terminated before completing, so the lint result is unknown; failing the step"
    STATUS=1
  fi
  exit "$STATUS"
}

trap gedlint_guard_exit EXIT
