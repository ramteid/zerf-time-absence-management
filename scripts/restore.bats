#!/usr/bin/env bats
# Unit tests for scripts/restore.sh helper functions.
#
# Run with: bats scripts/restore.bats
# Requires: bats-core

setup() {
  export RESTORE_LIB_ONLY=1
  # shellcheck source=/dev/null
  source "$(dirname "$BATS_TEST_FILENAME")/restore.sh"
}

@test "resolve_selection_index: converts valid decimal boundaries" {
  run resolve_selection_index "1" "12"
  [ "$status" -eq 0 ]
  [ "$output" = "0" ]

  run resolve_selection_index "10" "12"
  [ "$status" -eq 0 ]
  [ "$output" = "9" ]

  run resolve_selection_index "12" "12"
  [ "$status" -eq 0 ]
  [ "$output" = "11" ]

  run resolve_selection_index "999999999999999999" "999999999999999999"
  [ "$status" -eq 0 ]
  [ "$output" = "999999999999999998" ]
}

@test "resolve_selection_index: rejects zero and out-of-range values" {
  run resolve_selection_index "0" "12"
  [ "$status" -eq 1 ]

  run resolve_selection_index "13" "12"
  [ "$status" -eq 1 ]
}

@test "resolve_selection_index: rejects leading zeroes" {
  run resolve_selection_index "01" "12"
  [ "$status" -eq 1 ]

  run resolve_selection_index "08" "12"
  [ "$status" -eq 1 ]

  run resolve_selection_index "010" "12"
  [ "$status" -eq 1 ]
}

@test "resolve_selection_index: rejects oversized numeric input" {
  run resolve_selection_index "18446744073709551617" "2"
  [ "$status" -eq 1 ]

  run resolve_selection_index "999999999999999999999999999999999999999999" "12"
  [ "$status" -eq 1 ]
}

@test "resolve_selection_index: rejects non-numeric input" {
  run resolve_selection_index "" "12"
  [ "$status" -eq 1 ]

  run resolve_selection_index "1a" "12"
  [ "$status" -eq 1 ]

  run resolve_selection_index "-1" "12"
  [ "$status" -eq 1 ]
}

@test "resolve_selection_index: rejects invalid option counts" {
  run resolve_selection_index "1" "0"
  [ "$status" -eq 1 ]

  run resolve_selection_index "1" "not-a-number"
  [ "$status" -eq 1 ]

  run resolve_selection_index "1" "9999999999999999999"
  [ "$status" -eq 1 ]
}
