<script lang="ts">
  import { onMount, tick } from 'svelte';
  import Toast from '$lib/Toast.svelte';
  import {
    ApiError,
    createTask,
    statusPage,
    removeTask,
    updateTask,
    type Task
  } from '$lib/api';
  import { collectUntilMatch, ScanCancelled } from '$lib/status-scan';

  type Status = 'open' | 'completed';
  type ScanState = {
    cursor: string | null;
    exhausted: boolean;
    inspected: number;
    requested: boolean;
    scanning: boolean;
    error: string;
    generation: number;
  };
  const newScan = (): ScanState => ({
    cursor: null, exhausted: false, inspected: 0, requested: false,
    scanning: false, error: '', generation: 0
  });
  type UndoChange = {
    current: Task; // confirmed post-mutation revision; never silently retry
    inverse: { title: string } | { completed: boolean };
  };
  type UndoOperation = { changes: UndoChange[]; partialFailures: number };

  let tasks = $state<Task[]>([]);
  let openScan = $state<ScanState>(newScan());
  let completedScan = $state<ScanState>(newScan());
  let completedExpanded = $state(false);
  // Imperative cancellation ownership belongs to each live status request.
  let openAbort: AbortController | null = null;
  let completedAbort: AbortController | null = null;
  let loading = $state(true);
  let refreshing = $state(false);
  let refreshNotice = $state('');
  let lastCheckedAt = $state<Date | null>(null);
  let loadingMore = $state(false);
  let creating = $state(false);
  let actingId = $state<string | null>(null);
  let newTitle = $state('');
  let fieldError = $state('');
  let editingId = $state<string | null>(null);
  let editDraft = $state('');
  let editError = $state('');
  let confirmingDeleteId = $state<string | null>(null);
  let errorMessage = $state('');
  let conflicted = $state(false);
  let uncertain = $state(false);
  let toastMessage = $state('');
  let toastId = $state(0);
  let undoOperation = $state<UndoOperation | null>(null);
  let selectMode = $state(false);
  let selectedIds = $state<string[]>([]);
  let selectionAnchor = $state<string | null>(null);
  let batchRunning = $state(false);
  let batchProgress = $state('');
  let batchSummary = $state('');
  let batchFailures = $state<{ title: string; reason: string }[]>([]);

  // Each count is explicitly of loaded task rows, never global matches.
  const openTasks = $derived(tasks.filter((task) => !task.completed));
  const completedTasks = $derived(tasks.filter((task) => task.completed));
  // Shift-range and visible-only selection follow the rendered section order,
  // not the interleaved UUID order of the backing row cache.
  const visible = $derived([
    ...openTasks,
    ...(completedExpanded ? completedTasks : [])
  ]);
  const openCount = $derived(openTasks.length);
  const selectedVisible = $derived(visible.filter((task) => selectedIds.includes(task.id)));
  const lastCheckedLabel = $derived(
    lastCheckedAt === null
      ? 'Never'
      : lastCheckedAt.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' })
  );

  onMount(() => {
    void refresh();
  });

  function clearFeedback() {
    errorMessage = '';
    conflicted = false;
    uncertain = false;
    toastMessage = '';
    undoOperation = null;
    batchSummary = '';
    batchFailures = [];
  }

  function notify(message: string, undo: UndoOperation | null = null) {
    toastMessage = message;
    undoOperation = undo;
    toastId += 1;
  }

  function report(error: unknown) {
    toastMessage = '';
    conflicted = error instanceof ApiError && error.code === 'revision_conflict';
    uncertain = error instanceof ApiError && error.code === 'outcome_unknown';
    errorMessage = uncertain
      ? 'This change may have been applied. Check current tasks before repeating the action.'
      : error instanceof ApiError
        ? error.message
        : 'An unexpected error occurred. Please try again.';
  }

  function checkCurrentTasks() {
    // Only an explicit user choice may discard an uncertain edit draft.
    // A GET refresh is reconciliation, never a mutation retry.
    if (uncertain && editingId !== null) {
      editingId = null;
      editError = '';
    }
    void refresh();
  }

  function sortedUnique(records: Task[]): Task[] {
    return [...new Map(records.map((task) => [task.id, task])).values()]
      .sort((a, b) => a.id.localeCompare(b.id));
  }

  function invalidateStatusCursors() {
    openAbort?.abort();
    completedAbort?.abort();
    openAbort = null;
    completedAbort = null;
    // A live UUID seek is not a snapshot: confirmed writes may reclassify a
    // record behind either cursor. Keep visible confirmed rows and Undo, but
    // never suggest the old continuation is still a complete status view.
    for (const state of [openScan, completedScan]) {
      state.generation += 1;
      state.cursor = null;
      state.exhausted = false;
      state.scanning = false;
      state.requested = false;
    }
  }

  function stopScan(status: Status) {
    const state = status === 'open' ? openScan : completedScan;
    if (!state.scanning) return;
    if (status === 'open') {
      openAbort?.abort();
      openAbort = null;
    } else {
      completedAbort?.abort();
      completedAbort = null;
    }
    state.generation += 1;
    state.scanning = false;
    state.cursor = null;
    state.exhausted = false;
    state.error = 'Scan stopped. Check again from the beginning to avoid reusing an uncertain cursor.';
  }

  async function scanStatus(status: Status, fresh: boolean): Promise<boolean> {
    const state = status === 'open' ? openScan : completedScan;
    if (state.scanning || (!fresh && (state.exhausted || !state.cursor))) return false;
    const generation = ++state.generation;
    const controller = new AbortController();
    if (status === 'open') openAbort = controller;
    else completedAbort = controller;
    const completed = status === 'completed';
    const initialCursor = fresh ? null : state.cursor;
    const previousInspected = fresh ? 0 : state.inspected;
    let replaced = false;
    state.requested = true;
    state.scanning = true;
    state.error = '';
    if (fresh) {
      state.cursor = null;
      state.exhausted = false;
      state.inspected = 0;
    }
    try {
      const result = await collectUntilMatch(
        completed,
        initialCursor,
        (wanted, cursor) => statusPage(
          wanted,
          cursor,
          (input, init) => fetch(input, { ...init, signal: controller.signal })
        ),
        (progress) => {
          if (state.generation !== generation) return;
          if (fresh && !replaced) {
            tasks = tasks.filter((task) => task.completed !== completed);
            replaced = true;
          }
          state.inspected = previousInspected + progress.inspected;
          state.cursor = progress.nextCursor;
          state.exhausted = progress.exhausted;
          if (progress.records.length > 0) {
            tasks = sortedUnique([...tasks, ...progress.records]);
          }
        },
        () => state.generation === generation
      );
      if (state.generation !== generation) return false;
      state.cursor = result.nextCursor;
      state.exhausted = result.exhausted;
      lastCheckedAt = new Date();
      return true;
    } catch (error) {
      if (state.generation !== generation || error instanceof ScanCancelled) return false;
      // The last GET may have consumed its one-use cursor before its reply
      // disappeared. A retry with that token is unsafe; offer a fresh scan.
      state.cursor = null;
      state.exhausted = false;
      state.error = error instanceof ApiError
        ? error.message + ' Start a fresh scan to check current tasks.'
        : 'Could not check this task section. Start a fresh scan.';
      return false;
    } finally {
      if (state.generation === generation) {
        state.scanning = false;
        if (status === 'open') openAbort = null;
        else completedAbort = null;
      }
    }
  }

  async function refresh() {
    // Do not discard an unsaved edit, uncertain write or in-flight command.
    if (refreshing || loadingMore || creating || batchRunning || actingId !== null ||
      (editingId !== null && !conflicted)) return;
    refreshing = true;
    refreshNotice = 'Checking tasks';
    clearFeedback();
    const openSucceeded = await scanStatus('open', true);
    const completedSucceeded = completedExpanded
      ? await scanStatus('completed', true)
      : true;
    if (openSucceeded && completedSucceeded) {
      editingId = null;
      editError = '';
      confirmingDeleteId = null;
      clearSelection();
      selectMode = false;
      refreshNotice = 'Tasks checked';
    } else {
      refreshNotice = 'Task check incomplete';
    }
    loading = false;
    refreshing = false;
  }

  async function loadMore(status: Status) {
    if (loadingMore || refreshing || batchRunning || actingId !== null || creating ||
        editingId !== null) return;
    loadingMore = true;
    try {
      await scanStatus(status, status === 'open'
        ? !openScan.requested || !openScan.cursor
        : !completedScan.requested || !completedScan.cursor);
    } finally {
      loadingMore = false;
    }
  }

  async function toggleCompleted() {
    if (refreshing || loadingMore || creating || batchRunning || actingId !== null ||
        editingId !== null) return;
    completedExpanded = !completedExpanded;
    if (!completedExpanded) {
      stopScan('completed');
      completedScan.requested = false;
      completedScan.cursor = null;
      completedScan.exhausted = false;
      clearSelection();
      return;
    }
    loadingMore = true;
    try {
      await scanStatus('completed', true);
    } finally {
      loadingMore = false;
    }
  }

  async function add() {
    if (creating || refreshing || loadingMore || batchRunning) return;
    fieldError = '';
    if (!newTitle.trim()) {
      fieldError = 'Enter a task title.';
      return;
    }
    const submittedTitle = newTitle;
    creating = true;
    clearFeedback();
    try {
      const task = await createTask(submittedTitle);
      tasks = sortedUnique([...tasks, task]);
      invalidateStatusCursors();
      // A user may start typing another task while this request is pending.
      // Clear only the submitted draft; never discard newer input.
      if (newTitle === submittedTitle) newTitle = '';
      notify('Task created');
    } catch (error) {
      if (error instanceof ApiError && error.code === 'invalid_request') {
        fieldError = error.message;
      } else {
        report(error);
      }
    } finally {
      creating = false;
    }
  }

  async function startEdit(task: Task) {
    editingId = task.id;
    editDraft = task.title;
    editError = '';
    confirmingDeleteId = null;
    clearFeedback();
    await tick();
    document.getElementById('edit-' + task.id)?.focus();
  }

  async function cancelEdit(id: string) {
    editingId = null;
    editError = '';
    await tick();
    document.getElementById('edit-action-' + id)?.focus();
  }

  async function saveEdit(task: Task) {
    if (actingId || refreshing || loadingMore || batchRunning) return;
    editError = '';
    if (!editDraft.trim()) {
      editError = 'Enter a task title.';
      return;
    }
    actingId = task.id;
    clearFeedback();
    try {
      const updated = await updateTask(task, { title: editDraft });
      tasks = tasks.map((value) => value.id === updated.id ? updated : value);
      invalidateStatusCursors();
      editingId = null;
      // Re-enable the action trigger before restoring keyboard focus.
      actingId = null;
      notify('Task updated', {
        changes: [{ current: updated, inverse: { title: task.title } }],
        partialFailures: 0
      });
      await tick();
      document.getElementById('edit-action-' + task.id)?.focus();
    } catch (error) {
      if (error instanceof ApiError && error.code === 'invalid_request') {
        editError = error.message;
      } else {
        report(error);
      }
    } finally {
      actingId = null;
    }
  }

  async function toggle(task: Task) {
    if (actingId || refreshing || loadingMore || batchRunning) return;
    actingId = task.id;
    clearFeedback();
    try {
      const updated = await updateTask(task, { completed: !task.completed });
      tasks = sortedUnique([...tasks.filter((value) => value.id !== updated.id), updated]);
      invalidateStatusCursors();
      notify(updated.completed ? 'Task completed' : 'Task reopened', {
        changes: [{ current: updated, inverse: { completed: task.completed } }],
        partialFailures: 0
      });
      // The completed row may disappear into a collapsed section.
      // Transfer keyboard focus to its disclosure instead of losing it.
      actingId = null;
      await tick();
      if (updated.completed && !completedExpanded) {
        document.getElementById('completed-toggle')?.focus();
      } else {
        document.getElementById('task-check-' + updated.id)?.focus();
      }
    } catch (error) {
      report(error);
    } finally {
      actingId = null;
    }
  }

  async function remove(task: Task) {
    if (actingId || refreshing || loadingMore || batchRunning) return;
    actingId = task.id;
    clearFeedback();
    try {
      await removeTask(task);
      tasks = tasks.filter((value) => value.id !== task.id);
      invalidateStatusCursors();
      confirmingDeleteId = null;
      notify('Task removed');
    } catch (error) {
      report(error);
    } finally {
      actingId = null;
    }
  }

  function clearSelection() {
    selectedIds = [];
    selectionAnchor = null;
  }

  function exitSelection() {
    if (batchRunning) return;
    selectMode = false;
    clearSelection();
    batchSummary = '';
    batchFailures = [];
  }

  function enterSelection() {
    if (batchRunning || refreshing || loadingMore || creating ||
        actingId !== null || editingId !== null || confirmingDeleteId !== null) return;
    selectMode = true;
    clearSelection();
    batchSummary = '';
    batchFailures = [];
  }

  function selectTask(id: string, range = false) {
    if (batchRunning) return;
    const ids = visible.map((task) => task.id);
    const index = ids.indexOf(id);
    if (index === -1) return;
    if (range && selectionAnchor !== null && ids.includes(selectionAnchor)) {
      const first = ids.indexOf(selectionAnchor);
      const lo = Math.min(first, index);
      const hi = Math.max(first, index);
      selectedIds = [...new Set([...selectedIds, ...ids.slice(lo, hi + 1)])];
    } else {
      selectedIds = selectedIds.includes(id)
        ? selectedIds.filter((selected) => selected !== id)
        : [...selectedIds, id];
      selectionAnchor = id;
    }
    batchSummary = '';
    batchFailures = [];
  }

  function selectAllVisible() {
    if (batchRunning) return;
    selectedIds = visible.map((task) => task.id);
    selectionAnchor = visible.length > 0 ? visible[0].id : null;
    batchSummary = '';
    batchFailures = [];
  }

  function onTaskTitle(event: MouseEvent, task: Task) {
    if (selectMode) {
      selectTask(task.id, event.shiftKey);
    } else if (event.shiftKey || event.ctrlKey || event.metaKey) {
      // Desktop modifier-click enters selection; ordinary title click edits.
      enterSelection();
      if (selectMode) selectTask(task.id);
    } else {
      void startEdit(task);
    }
  }

  async function setBatchCompletion(completed: boolean) {
    if (!selectMode || batchRunning || actingId !== null || refreshing ||
        loadingMore || creating || editingId !== null) return;
    const snapshot = visible.filter((task) => selectedIds.includes(task.id));
    if (snapshot.length === 0) return;

    batchRunning = true;
    clearFeedback();
    let changed = 0;
    let unchanged = 0;
    const failedIds: string[] = [];
    const failures: { title: string; reason: string }[] = [];
    const successful: UndoChange[] = [];
    try {
      for (let index = 0; index < snapshot.length; index++) {
        const task = snapshot[index];
        batchProgress = 'Updating ' + (index + 1) + ' of ' + snapshot.length + ' selected tasks…';
        if (task.completed === completed) {
          unchanged++;
          continue;
        }
        try {
          // Sequential, revision-checked commands: no atomic batch, no automatic retry.
          const updated = await updateTask(task, { completed });
          tasks = sortedUnique([...tasks.filter((value) => value.id !== updated.id), updated]);
          invalidateStatusCursors();
          successful.push({ current: updated, inverse: { completed: task.completed } });
          changed++;
        } catch (error) {
          failedIds.push(task.id);
          failures.push({
            title: task.title,
            reason: error instanceof ApiError ? error.message : 'Unexpected request failure'
          });
        }
      }

      if (failures.length > 0) {
        selectedIds = failedIds;
        selectionAnchor = failedIds[0] ?? null;
        batchFailures = failures;
        undoOperation = successful.length > 0
          ? { changes: successful, partialFailures: failures.length }
          : null;
        batchSummary = changed + ' changed, ' + unchanged + ' already ' +
          (completed ? 'done' : 'open') + ', ' + failures.length +
          ' failed. Failed tasks remain selected. Refresh to reconcile uncertain outcomes before retrying.';
      } else {
        selectMode = false;
        clearSelection();
        notify(
          changed === 0
            ? 'Selected tasks already ' + (completed ? 'done' : 'open')
            : changed + (changed === 1 ? ' task ' : ' tasks ') +
              (completed ? 'marked done' : 'reopened'),
          successful.length > 0 ? { changes: successful, partialFailures: 0 } : null
        );
      }
    } finally {
      batchProgress = '';
      batchRunning = false;
    }
  }

  async function undoLastAction() {
    const action = undoOperation;
    if (!action || action.changes.length === 0 || batchRunning || refreshing ||
        loadingMore || creating || actingId !== null || editingId !== null) return;

    // Consume this opportunity before issuing any inverse mutation, so an
    // uncertain response can never trigger an automatic second attempt.
    undoOperation = null;
    toastMessage = '';
    batchRunning = true;
    const failures: { title: string; reason: string }[] = [];
    let reversed = 0;
    try {
      for (let index = 0; index < action.changes.length; index++) {
        const change = action.changes[index];
        batchProgress = 'Undoing ' + (index + 1) + ' of ' + action.changes.length + ' changes…';
        try {
          // The post-command revision fences newer edits from another client.
          const updated = await updateTask(change.current, change.inverse);
          tasks = sortedUnique([...tasks.filter((task) => task.id !== updated.id), updated]);
          invalidateStatusCursors();
          reversed++;
        } catch (error) {
          failures.push({
            title: change.current.title,
            reason: error instanceof ApiError ? error.message : 'Unexpected request failure'
          });
        }
      }
      if (failures.length > 0) {
        batchFailures = [...batchFailures, ...failures];
        batchSummary = reversed + ' reversed, ' + failures.length +
          ' Undo changes failed. Refresh to reconcile before further updates; failed requests may have committed.';
      } else if (action.partialFailures > 0) {
        // Original unsuccessful batch items remain selected for reconciliation.
        batchSummary = 'Undid ' + reversed + ' successful changes. ' +
          action.partialFailures + ' original batch operations failed and remain selected. ' +
          'Refresh before retrying those tasks.';
        notify(reversed === 1 ? 'Change undone' : reversed + ' changes undone');
      } else {
        batchSummary = '';
        batchFailures = [];
        notify(reversed === 1 ? 'Change undone' : reversed + ' changes undone');
      }
    } finally {
      batchProgress = '';
      batchRunning = false;
    }
    if (failures.length > 0) {
      await tick();
      document.querySelector<HTMLButtonElement>('.batch-result .text-action')?.focus();
    }
  }

  async function confirmDelete(id: string) {
    confirmingDeleteId = id;
    await tick();
    document.getElementById('delete-confirm-' + id)?.focus();
  }

  async function cancelDelete(id: string) {
    confirmingDeleteId = null;
    await tick();
    document.getElementById('delete-action-' + id)?.focus();
  }

  function onWindowKeydown(event: KeyboardEvent) {
    if (batchRunning) return;
    // A focused selection checkbox is not an editable text field. Ctrl/Cmd+A
    // must select visible loaded tasks there, but preserve native text editing.
    const textEditing = event.target instanceof HTMLTextAreaElement ||
      (event.target instanceof HTMLInputElement &&
        !['checkbox', 'radio', 'button', 'submit'].includes(event.target.type)) ||
      (event.target instanceof HTMLElement && event.target.isContentEditable);
    if (selectMode && (event.ctrlKey || event.metaKey) &&
        event.key.toLowerCase() === 'a' && !textEditing) {
      event.preventDefault();
      selectAllVisible();
      return;
    }
    if (event.key !== 'Escape') return;
    if (confirmingDeleteId) {
      event.preventDefault();
      void cancelDelete(confirmingDeleteId);
    } else if (editingId) {
      event.preventDefault();
      void cancelEdit(editingId);
    } else if (selectMode) {
      event.preventDefault();
      exitSelection();
    }
  }
</script>

<svelte:head>
  <title>Todo — Riesel</title>
  <meta name="description" content="Manage tasks with the local Riesel Todo application." />
</svelte:head>

<svelte:window onkeydown={onWindowKeydown} />

<div class="app-shell">
  <header class="topbar">
    <div class="topbar-inner">
      <div class="brand">
        <span class="brand-mark" aria-hidden="true">R</span>
        <span>Riesel <span class="brand-divider">/</span> Todo</span>
      </div>
      <span class="app-context">Local task manager</span>
    </div>
  </header>

  <main class="content">
    <header class="page-heading">
      <div>
        <h1 id="page-title">Tasks</h1>
      </div>
    </header>

    <section class="panel" aria-label="Task management">
      <form class="new-task" onsubmit={(event) => { event.preventDefault(); void add(); }}>
        <label for="new-title">New task</label>
        <div class="new-task-row">
          <input
            id="new-title"
            name="title"
            type="text"
            bind:value={newTitle}
            oninput={() => { fieldError = ''; }}
            aria-invalid={fieldError !== ''}
            aria-describedby={fieldError ? 'new-title-error' : undefined}
            autocomplete="off"
            placeholder="What needs doing?"
            required
          />
          <button type="submit" class="primary" disabled={creating || actingId !== null || refreshing || loadingMore || batchRunning}>
            {creating ? 'Adding…' : 'Add task'}
          </button>
        </div>
        {#if fieldError}
          <p class="field-error" id="new-title-error" role="alert">{fieldError}</p>
        {/if}
      </form>

      <div class="toolbar">
        <div class="refresh-group">
          <button
            type="button"
            class="refresh-trigger"
            aria-label="Refresh tasks"
            title="Refresh tasks"
            disabled={refreshing || loading || loadingMore || creating ||
              actingId !== null || batchRunning || (editingId !== null && !conflicted)}
            onclick={() => void refresh()}
          >
            <svg
              class="refresh-icon"
              class:spinning={refreshing}
              xmlns="http://www.w3.org/2000/svg"
              width="20"
              height="20"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
              stroke-linecap="round"
              stroke-linejoin="round"
              aria-hidden="true"
            >
              <path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8"/>
              <path d="M21 3v5h-5"/>
              <path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16"/>
              <path d="M8 16H3v5"/>
            </svg>
          </button>
          <div class="refresh-details">
            <span class="refresh-caption">{refreshing ? 'Checking…' : 'Last checked'}</span>
            {#if lastCheckedAt}
              <time
                class="refresh-time"
                datetime={lastCheckedAt.toISOString()}
                title={lastCheckedAt.toLocaleString()}
              >{lastCheckedLabel}</time>
            {:else}
              <span class="refresh-time">Never</span>
            {/if}
          </div>
          <span class="sr-only" aria-live="polite">{refreshNotice}</span>
        </div>
        <p class="count" aria-label={tasks.length + ' tasks loaded, ' + openCount + ' open in loaded tasks'}>
          {tasks.length} loaded <span aria-hidden="true">·</span> {openCount} open
        </p>
        <button
          type="button"
          class="select-mode-trigger"
          aria-pressed={selectMode}
          disabled={loading || refreshing || loadingMore || creating || actingId !== null ||
            batchRunning || editingId !== null || confirmingDeleteId !== null}
          onclick={() => selectMode ? exitSelection() : enterSelection()}
        >{selectMode ? 'Cancel selection' : 'Select tasks'}</button>
      </div>

      {#if selectMode}
        <div class="selection-bar" role="group" aria-label="Batch task actions">
          <div class="selection-info">
            <strong>{selectedVisible.length} selected</strong>
            <span class="selection-scope">from {visible.length} displayed, loaded tasks</span>
          </div>
          <div class="selection-actions">
            <button type="button" class="small" disabled={batchRunning || visible.length === 0}
              onclick={selectAllVisible}>Select all loaded</button>
            <button type="button" class="small" disabled={batchRunning || selectedVisible.length === 0}
              onclick={clearSelection}>Clear</button>
            <button type="button" class="small primary" disabled={batchRunning || selectedVisible.length === 0}
              onclick={() => void setBatchCompletion(true)}>Mark done</button>
            <button type="button" class="small" disabled={batchRunning || selectedVisible.length === 0}
              onclick={() => void setBatchCompletion(false)}>Reopen</button>
          </div>
          {#if batchProgress}
            <span class="selection-progress" role="status" aria-live="polite">{batchProgress}</span>
          {/if}
        </div>
      {/if}

      {#if batchSummary}
        <div class="feedback error batch-result" role="alert">
          <div>
            <p>{batchSummary}</p>
            <ul>
              {#each batchFailures as failure}
                <li>{failure.title}: {failure.reason}</li>
              {/each}
            </ul>
          </div>
          {#if undoOperation !== null && undoOperation.partialFailures > 0}
            <button type="button" class="text-action" disabled={batchRunning}
              onclick={() => void undoLastAction()}>Undo successful changes</button>
          {/if}
          <button type="button" class="text-action" disabled={batchRunning} onclick={() => void refresh()}>Refresh tasks</button>
        </div>
      {/if}

      {#if errorMessage}
        <div class="feedback error" role="alert">
          <span>{errorMessage}</span>
          {#if editingId !== null && !conflicted && !uncertain}
            <button type="button" class="text-action" onclick={() => clearFeedback()}>
              Dismiss
            </button>
          {:else}
            <button type="button" class="text-action" onclick={checkCurrentTasks}>
              {conflicted
                ? 'Reload current tasks'
                : uncertain
                  ? editingId !== null
                    ? 'Discard draft and check tasks'
                    : 'Check current tasks'
                  : 'Try again'}
            </button>
          {/if}
        </div>
      {/if}

      {#snippet taskList(rows: Task[], label: string)}
        <ul class="task-list" aria-label={label}>
          {#each rows as task (task.id)}
            <li class="task" class:done={task.completed} class:selected={selectMode && selectedIds.includes(task.id)}>
              <label class="task-check-control">
                {#if selectMode}
                  <input
                    type="checkbox"
                    class="task-checkbox selection-checkbox"
                    checked={selectedIds.includes(task.id)}
                    disabled={batchRunning}
                    aria-label={'Select ' + task.title}
                    onclick={(event) => selectTask(task.id, event.shiftKey)}
                  />
                {:else}
                  <input
                    type="checkbox"
                    class="task-checkbox"
                    id={'task-check-' + task.id}
                    checked={task.completed}
                    disabled={actingId !== null || refreshing || loadingMore || batchRunning}
                    aria-label={'Mark ' + task.title + (task.completed ? ' incomplete' : ' complete')}
                    onchange={() => void toggle(task)}
                  />
                {/if}
              </label>

              <div class="task-content">
                {#if editingId === task.id}
                  <form class="edit-form" onsubmit={(event) => { event.preventDefault(); void saveEdit(task); }}>
                    <label class="sr-only" for={'edit-' + task.id}>Edit task title</label>
                    <input
                      id={'edit-' + task.id}
                      type="text"
                      bind:value={editDraft}
                      oninput={() => { editError = ''; }}
                      aria-invalid={editError !== ''}
                      aria-describedby={editError ? 'edit-error-' + task.id : undefined}
                      required
                    />
                    <button type="submit" class="small primary" disabled={actingId !== null || refreshing || loadingMore || batchRunning}>Save</button>
                    <button type="button" class="small" onclick={() => void cancelEdit(task.id)}>Cancel</button>
                    {#if editError}
                      <p class="field-error edit-error" id={'edit-error-' + task.id} role="alert">{editError}</p>
                    {/if}
                  </form>
                {:else}
                  <button
                    type="button"
                    class="task-title task-title-button"
                    disabled={actingId !== null || refreshing || loadingMore || batchRunning}
                    aria-label={selectMode ? 'Select ' + task.title + ' by title' : 'Edit ' + task.title + ' title'}
                    onclick={(event) => onTaskTitle(event, task)}
                  >{task.title}</button>
                {/if}
              </div>

              {#if editingId !== task.id && !selectMode}
                <div class="task-action-area">
                  {#if confirmingDeleteId === task.id}
                    <div class="delete-confirm" role="group" aria-label={'Confirm permanent deletion of ' + task.title}>
                      <span>Delete permanently?</span>
                      <button
                        id={'delete-confirm-' + task.id}
                        type="button"
                        class="small danger"
                        disabled={actingId !== null || refreshing || loadingMore || batchRunning}
                        onclick={() => void remove(task)}
                      >Delete</button>
                      <button type="button" class="small" onclick={() => void cancelDelete(task.id)}>Cancel</button>
                    </div>
                  {:else}
                    <div class="row-actions">
                      <button
                        id={'edit-action-' + task.id}
                        type="button"
                        class="icon-action"
                        disabled={actingId !== null || refreshing || loadingMore || batchRunning}
                        aria-label={'Edit ' + task.title}
                        title="Edit task"
                        onclick={() => void startEdit(task)}
                      ><svg aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 20h9"/><path d="M16.5 3.5a2.12 2.12 0 0 1 3 3L9 17l-4 1 1-4 10.5-10.5Z"/></svg></button>
                      <button
                        id={'delete-action-' + task.id}
                        type="button"
                        class="icon-action danger-icon"
                        disabled={actingId !== null || refreshing || loadingMore || batchRunning}
                        aria-label={'Delete ' + task.title}
                        title="Delete task permanently (confirmation required)"
                        onclick={() => void confirmDelete(task.id)}
                      ><svg aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 6h18"/><path d="M8 6V4h8v2"/><path d="M5 6l1 15h12l1-15"/><path d="M10 10v7"/><path d="M14 10v7"/></svg></button>
                    </div>
                  {/if}
                </div>
              {/if}
            </li>
          {/each}
        </ul>
      {/snippet}

      <section class="task-section" aria-labelledby="open-tasks-heading">
        <div class="section-heading">
          <h2 id="open-tasks-heading">Open tasks</h2>
          <span class="count">{openTasks.length} shown</span>
        </div>
        {#if openScan.scanning}
          <div class="section-progress" role="status">
            Checking open tasks · {openScan.inspected} records inspected
            <button type="button" class="text-action" onclick={() => stopScan('open')}>Stop checking</button>
          </div>
        {:else if openScan.error}
          <div class="section-message" role="alert">
            {openScan.error}
            <button type="button" class="text-action" onclick={() => void scanStatus('open', true)}>Check from start</button>
          </div>
        {:else if openTasks.length === 0 && openScan.exhausted}
          <div class="placeholder">
            <p class="empty-title">No open tasks found</p>
            <p>The completed live scan found no open tasks. Changes from other clients appear after refresh.</p>
          </div>
        {:else if openTasks.length === 0 && openScan.requested && !openScan.exhausted}
          <p class="section-message" role="status">No open tasks in the records checked yet. More records remain.</p>
        {:else if loading && openTasks.length === 0}
          <p class="placeholder" role="status">Loading tasks…</p>
        {:else if openTasks.length === 0 && !openScan.requested}
          <p class="section-message">Open tasks changed. Refresh to check the current list.</p>
        {/if}
        {@render taskList(openTasks, 'Open tasks')}
        <div class="panel-footer">
          {#if !openScan.requested && !openScan.scanning}
            <button type="button" class="secondary" disabled={loadingMore || refreshing || creating || actingId !== null || batchRunning || editingId !== null} onclick={() => void scanStatus('open', true)}>Check current open tasks</button>
          {:else if openScan.cursor && !openScan.exhausted && !openScan.scanning}
            <button type="button" class="secondary" disabled={loadingMore || refreshing || creating || actingId !== null || batchRunning} onclick={() => void loadMore('open')}>Continue checking open tasks</button>
          {:else if openScan.exhausted}
            <span>Open scan complete for now · {openScan.inspected} records inspected</span>
          {/if}
        </div>
      </section>

      <section class="task-section completed-section" aria-labelledby="completed-tasks-heading">
        <div class="section-heading">
          <h2 id="completed-tasks-heading">
            <button id="completed-toggle" type="button" class="section-toggle"
              aria-expanded={completedExpanded} aria-controls="completed-tasks-content"
              disabled={refreshing || loadingMore || batchRunning || actingId !== null || editingId !== null}
              onclick={() => void toggleCompleted()}>
              <span aria-hidden="true">{completedExpanded ? '▾' : '▸'}</span>
              Completed
            </button>
          </h2>
          {#if completedExpanded}<span class="count">{completedTasks.length} shown</span>{/if}
        </div>
        <div id="completed-tasks-content" hidden={!completedExpanded}>
          {#if completedExpanded}
            {#if completedScan.scanning}
              <div class="section-progress" role="status">
                Checking completed tasks · {completedScan.inspected} records inspected
                <button type="button" class="text-action" onclick={() => stopScan('completed')}>Stop checking</button>
              </div>
            {:else if completedScan.error}
              <div class="section-message" role="alert">
                {completedScan.error}
                <button type="button" class="text-action" onclick={() => void scanStatus('completed', true)}>Check from start</button>
              </div>
            {:else if completedTasks.length === 0 && completedScan.exhausted}
              <p class="section-message">No completed tasks found in the completed scan.</p>
            {:else if completedTasks.length === 0 && completedScan.requested}
              <p class="section-message" role="status">No completed tasks in the records checked yet. More records remain.</p>
            {/if}
            {@render taskList(completedTasks, 'Completed tasks')}
            <div class="panel-footer">
              {#if completedScan.cursor && !completedScan.exhausted && !completedScan.scanning}
                <button type="button" class="secondary" disabled={loadingMore || refreshing || creating || actingId !== null || batchRunning} onclick={() => void loadMore('completed')}>Continue checking completed tasks</button>
              {:else if completedScan.exhausted}
                <span>Completed scan complete for now · {completedScan.inspected} records inspected</span>
              {/if}
            </div>
          {/if}
        </div>
      </section>

    </section>
    <p class="footnote">Tasks are stored locally. Changes from other clients appear after refresh.</p>
  </main>

  {#if toastMessage}
    <div class="toast-region">
      {#key toastId}
        <Toast
          message={toastMessage}
          timeout={undoOperation ? 7000 : 3200}
          actionLabel={undoOperation ? 'Undo' : undefined}
          onAction={undoOperation ? () => void undoLastAction() : undefined}
          actionDisabled={batchRunning || refreshing || loadingMore || actingId !== null}
          onDismiss={() => { toastMessage = ''; undoOperation = null; }}
        />
      {/key}
    </div>
  {/if}
</div>
