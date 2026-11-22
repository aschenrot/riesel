/** Browser transport for the accepted Todo /api/v1 contract. */
export interface Task {
  id: string;
  revision: string;
  title: string;
  completed: boolean;
}

export interface TaskPage {
  records: Task[];
  next_cursor: string | null;
}

/** A bounded, live native Query result. Empty may be partial, not exhausted. */
export interface StatusTaskPage extends TaskPage {
  exhausted: boolean;
  inspected: number;
}

interface Problem {
  error?: { code?: string; message?: string };
}

export class ApiError extends Error {
  code: string;
  status: number;

  constructor(code: string, status: number, message: string) {
    super(message);
    this.name = 'ApiError';
    this.code = code;
    this.status = status;
  }
}

const PREFIX = '/api/v1';

async function request<T>(
  path: string,
  init: RequestInit,
  fetcher: typeof fetch
): Promise<T> {
  // A failed GET is an unavailable observation. A failed mutation
  // transport may have crossed dispatch and applied an effect already.
  // The browser cannot infer rollback from a lost response.
  const mutation = init.method !== 'GET' && init.method !== 'HEAD';
  let response: Response;
  try {
    response = await fetcher(PREFIX + path, {
      mode: 'same-origin',
      credentials: 'omit',
      cache: 'no-store',
      ...init,
      headers: init.body !== undefined ? { 'Content-Type': 'application/json' } : undefined
    });
  } catch {
    throw mutation
      ? new ApiError(
          'outcome_unknown',
          0,
          'The change may have been applied. Check current tasks before trying again.'
        )
      : new ApiError('disconnected', 0, 'Cannot connect to the Todo backend.');
  }

  if (!response.ok) {
    const problem = (await response.json().catch(() => null)) as Problem | null;
    throw new ApiError(
      problem?.error?.code || 'request_failed',
      response.status,
      problem?.error?.message || 'The request could not be completed.'
    );
  }

  if (response.status === 204) {
    return undefined as T;
  }
  try {
    return (await response.json()) as T;
  } catch {
    throw mutation
      ? new ApiError(
          'outcome_unknown',
          response.status,
          'The change may have been applied, but its response could not be read. Check current tasks.'
        )
      : new ApiError('invalid_response', 0, 'The backend returned an invalid response.');
  }
}

/** Request a bounded page. A cursor is opaque, not a client-side offset. */
export function listPage(
  after: string | null = null,
  fetcher: typeof fetch = fetch
): Promise<TaskPage> {
  const query = new URLSearchParams({ limit: '32' });
  if (after) query.set('after', after);
  return request<TaskPage>('/tasks?' + query.toString(), { method: 'GET' }, fetcher);
}

/** Todo-only status selector: no arbitrary model, SQL, query budget or sort. */
export function statusPage(
  completed: boolean,
  cursor: string | null = null,
  fetcher: typeof fetch = fetch
): Promise<StatusTaskPage> {
  const query = new URLSearchParams({ completed: String(completed), limit: '32' });
  if (cursor !== null) query.set('cursor', cursor);
  return request<StatusTaskPage>('/tasks/query?' + query.toString(), { method: 'GET' }, fetcher);
}

export function createTask(
  title: string,
  fetcher: typeof fetch = fetch
): Promise<Task> {
  return request<Task>('/tasks', {
    method: 'POST',
    body: JSON.stringify({ title })
  }, fetcher);
}

export function updateTask(
  task: Task,
  change: { title: string } | { completed: boolean },
  fetcher: typeof fetch = fetch
): Promise<Task> {
  return request<Task>('/tasks/' + encodeURIComponent(task.id), {
    method: 'PATCH',
    body: JSON.stringify({ revision: task.revision, ...change })
  }, fetcher);
}

export function removeTask(
  task: Task,
  fetcher: typeof fetch = fetch
): Promise<void> {
  return request<void>('/tasks/' + encodeURIComponent(task.id), {
    method: 'DELETE',
    body: JSON.stringify({ revision: task.revision })
  }, fetcher);
}
