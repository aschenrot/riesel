import type { StatusTaskPage, Task } from './api';

export const MAX_STATUS_REQUESTS_PER_BURST = 10;
export const MAX_INSPECTED_PER_HTTP_REQUEST = 1024;

export interface ScanProgress {
  records: Task[];
  nextCursor: string | null;
  exhausted: boolean;
  inspected: number;
  requests: number;
}

export class ScanCancelled extends Error {
  constructor() {
    super('Status scan was stopped; start a fresh scan before reusing a continuation');
    this.name = 'ScanCancelled';
  }
}

/**
 * Bounded progressive discovery. A native Query can return zero matches while
 * still scanning; do not treat that as an empty collection or spin forever.
 * One-use provider tokens must be used sequentially, never replayed on error.
 */
export async function collectUntilMatch(
  completed: boolean,
  firstCursor: string | null,
  fetchPage: (completed: boolean, cursor: string | null) => Promise<StatusTaskPage>,
  onProgress: (progress: ScanProgress) => void = () => {},
  canContinue: () => boolean = () => true
): Promise<ScanProgress> {
  let cursor = firstCursor;
  let inspected = 0;
  let requests = 0;
  let exhausted = false;
  let records: Task[] = [];
  for (; requests < MAX_STATUS_REQUESTS_PER_BURST; ) {
    if (!canContinue()) throw new ScanCancelled();
    const page = await fetchPage(completed, cursor);
    // A timed-out/aborted read might have consumed a one-use provider token.
    // Do not retain or reuse its old cursor after the generation was revoked.
    if (!canContinue()) throw new ScanCancelled();
    if (!Array.isArray(page.records) || page.records.length > 32 ||
        !Number.isSafeInteger(page.inspected) || page.inspected < 0 ||
        page.inspected > MAX_INSPECTED_PER_HTTP_REQUEST ||
        typeof page.exhausted !== 'boolean' ||
        (page.next_cursor !== null && typeof page.next_cursor !== 'string') ||
        (!page.exhausted && !page.next_cursor) ||
        (page.exhausted && page.next_cursor !== null) ||
        (page.records.length === 0 && !page.exhausted && page.inspected === 0) ||
        page.records.some((record) => record.completed !== completed)) {
      throw new Error('Backend returned an inconsistent status Query page');
    }
    requests += 1;
    inspected += page.inspected;
    records = page.records;
    cursor = page.next_cursor;
    exhausted = page.exhausted;
    const progress = { records, nextCursor: cursor, exhausted, inspected, requests };
    onProgress(progress);
    if (records.length > 0 || exhausted) return progress;
  }
  return { records, nextCursor: cursor, exhausted, inspected, requests };
}
