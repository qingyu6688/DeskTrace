import type { ObservationTrack } from '../types/objects';

import { DAY_MS } from './objects';

export interface TrackGeometry {
  marks: number[];
  last: number | null;
  earlier: boolean;
  threshold: number | null;
  gaps: { left: number; width: number; state: 'gap' | 'unknown'; label: string }[];
  weekly: boolean;
}

function dayOverlapsWindow(day: string, start: number, end: number): boolean {
  const dayStart = Date.parse(`${day}T00:00:00+08:00`);
  return Number.isFinite(dayStart) && dayStart <= end && dayStart + DAY_MS > start;
}

export function windowedTrack(track: ObservationTrack, retentionDays: number): ObservationTrack {
  const end = Date.parse(track.windowEnd);
  const start = Math.max(Date.parse(track.windowStart), end - retentionDays * DAY_MS);
  if (!Number.isFinite(start) || !Number.isFinite(end)) return track;
  return {
    ...track,
    windowStart: new Date(start).toISOString(),
    usageDays: track.usageDays.filter((day): boolean => dayOverlapsWindow(day, start, end)),
    gaps: track.gaps.filter((gap): boolean => Date.parse(gap.startedAt) < end && (!gap.endedAt || Date.parse(gap.endedAt) > start)),
  };
}

export function trackGeometry(track: ObservationTrack, thresholdDays: number, width: number): TrackGeometry | null {
  const start = Date.parse(track.windowStart);
  const end = Date.parse(track.windowEnd);
  if (!Number.isFinite(start) || !Number.isFinite(end) || end <= start || width <= 0) return null;
  const range = end - start;
  const position = (point: number): number => Math.max(0, Math.min(100, (point - start) / range * 100));
  const weekly = range > 365 * DAY_MS;
  const days = [...new Set(track.usageDays)]
    .filter((day): boolean => dayOverlapsWindow(day, start, end))
    .map((day): number => Math.max(start, Math.min(end, Date.parse(`${day}T12:00:00+08:00`))));
  const groups = new Map<number, number>();
  for (const [index, point] of days.entries()) groups.set(weekly ? Math.floor((point - start) / (7 * DAY_MS)) : index, point);
  const gaps: TrackGeometry['gaps'] = [];
  const sorted = [...track.gaps].sort((a, b): number => Date.parse(a.startedAt) - Date.parse(b.startedAt));
  for (const gap of sorted) {
    const from = Math.max(start, Date.parse(gap.startedAt));
    const to = Math.min(end, gap.endedAt ? Date.parse(gap.endedAt) : end);
    if (!Number.isFinite(from) || !Number.isFinite(to) || to <= from) continue;
    const leftPx = Math.min(width - Math.min(2, width), Math.floor(position(from) * width / 100));
    const rightPx = Math.min(width, Math.max(leftPx + 2, Math.ceil(position(to) * width / 100)));
    const previous = gaps.at(-1);
    const left = leftPx / width * 100;
    const right = rightPx / width * 100;
    if (previous && previous.state === gap.state && left <= previous.left + previous.width) {
      previous.width = Math.max(previous.left + previous.width, right) - previous.left;
      if (!previous.label.includes(gap.reasonLabel)) previous.label += `、${gap.reasonLabel}`;
    } else gaps.push({ left, width: right - left, state: gap.state, label: gap.reasonLabel });
  }
  const cutoff = end - thresholdDays * DAY_MS;
  const last = track.lastUsedAt ? Date.parse(track.lastUsedAt) : null;
  return {
    marks: [...groups.values()].map(position), last: last !== null && Number.isFinite(last) ? position(last) : null,
    earlier: last !== null && last < start, threshold: cutoff > start ? position(cutoff) : null, gaps, weekly,
  };
}
