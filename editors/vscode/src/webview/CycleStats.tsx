/**
 * Scan cycle statistics display.
 */

import type { CycleInfo } from "../shared/types";
import { fmtUs } from "./util";

interface CycleStatsProps {
  info: CycleInfo | null;
}

export function CycleStats({ info }: CycleStatsProps) {
  if (!info) {
    return (
      <div class="stats">
        <span class="stat-label">Cycles:</span>
        <span class="stat-value">0</span>
        <span class="stat-label">Last:</span>
        <span class="stat-value">-</span>
        <span class="stat-label">Min:</span>
        <span class="stat-value">-</span>
        <span class="stat-label">Max:</span>
        <span class="stat-value">-</span>
        <span class="stat-label">Avg:</span>
        <span class="stat-value">-</span>
      </div>
    );
  }

  return (
    <div class="stats">
      <span class="stat-label">Cycles:</span>
      <span class="stat-value">{info.cycle_count.toLocaleString()}</span>
      <span class="stat-label">Last:</span>
      <span class="stat-value">{fmtUs(info.last_cycle_us)}</span>
      <span class="stat-label">Min:</span>
      <span class="stat-value">{fmtUs(info.min_cycle_us)}</span>
      <span class="stat-label">Max:</span>
      <span class="stat-value">{fmtUs(info.max_cycle_us)}</span>
      <span class="stat-label">Avg:</span>
      <span class="stat-value">{fmtUs(info.avg_cycle_us)}</span>
      {info.target_us != null && (
        <>
          <span class="stat-label">Target:</span>
          <span class="stat-value">{fmtUs(info.target_us)}</span>
          <span class="stat-label">Period:</span>
          <span class="stat-value">{fmtUs(info.last_period_us)}</span>
          <span class="stat-label">Jitter (max):</span>
          <span class="stat-value">{fmtUs(info.jitter_max_us)}</span>
        </>
      )}
    </div>
  );
}
