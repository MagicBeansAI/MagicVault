// Test orchestration only. No recipient data or production instrumentation.
import { performance } from 'node:perf_hooks';

export const INVESTIGATION_BUDGET_MS = 600_000;
export function investigationRounds(value) {
  if (typeof value !== 'string' || !/^(?:[1-9]|[1-9][0-9]|1[0-9]{2}|200)$/.test(value)) {
    throw new Error('process investigation requires 1..200 fresh trials');
  }
  return Number(value);
}

// Build/preflight time is separate. Every trial starts a fresh test executable,
// runtime and synthetic fixture; a thrown failure ends the loop without retry.
export function investigate(rounds, trial, report, now = () => performance.now()) {
  investigationRounds(String(rounds));
  const started = now();
  for (let round = 1; round <= rounds; round++) {
    const remaining = Math.floor(INVESTIGATION_BUDGET_MS - (now() - started));
    if (remaining <= 0) throw new Error(`process investigation budget exhausted after ${round - 1} completed trials`);
    report({ process_investigation_round: round, rounds });
    trial(Math.min(120_000, remaining));
    if (now() - started >= INVESTIGATION_BUDGET_MS) throw new Error(`process investigation budget exhausted after ${round} completed trials`);
  }
  return { outcome: 'inconclusive_no_reproduction', completed_trials: rounds, budget_ms: INVESTIGATION_BUDGET_MS };
}
