<script lang="ts">
  import { onMount, tick } from 'svelte';
  import Toast from '$lib/Toast.svelte';
  import {
    ApiError,
    createTask,
    listPage,
    removeTask,
    updateTask,
    type Task
  } from '$lib/api';

  type Filter = 'all' | 'open' | 'done';

  let tasks = $state<Task[]>([]);
  let nextCursor = $state<string | null>(null);
  let loading = $state(true);
  let loadingMore = $state(false);
  let creating = $state(false);
  let actingId = $state<string | null>(null);
  let filter = $state<Filter>('all');
  let newTitle = $state('');
  let fieldError = $state('');
  let editingId = $state<string | null>(null);
  let editDraft = $state('');
  let editError = $state('');
  let openMenuId = $state<string | null>(null);
  let confirmingDeleteId = $state<string | null>(null);
  let errorMessage = $state('');
  let conflicted = $state(false);
  let toastMessage = $state('');
  let toastId = $state(0);

  // Counts and filters intentionally describe only fetched pages.
  const visible = $derived(
    tasks.filter((task) =>
      filter === 'all' || (filter === 'done' ? task.completed : !task.completed)
    )
  );
  const openCount = $derived(tasks.filter((task) => !task.completed).length);

  onMount(() => {
    void refresh();
  });

  function clearFeedback() {
    errorMessage = '';
    conflicted = false;
    toastMessage = '';
  }

  function notify(message: string) {
    toastMessage = message;
    toastId += 1;
  }

  function report(error: unknown) {
    toastMessage = '';
    conflicted = error instanceof ApiError && error.code === 'revision_conflict';
    errorMessage = error instanceof ApiError
      ? error.message
      : 'An unexpected error occurred. Please try again.';
  }

  function sortedUnique(records: Task[]): Task[] {
    return [...new Map(records.map((task) => [task.id, task])).values()]
      .sort((a, b) => a.id.localeCompare(b.id));
  }

  async function refresh() {
    loading = true;
    clearFeedback();
    openMenuId = null;
    try {
      const page = await listPage();
      tasks = page.records;
      nextCursor = page.next_cursor;
    } catch (error) {
      report(error);
    } finally {
      loading = false;
    }
  }

  async function loadMore() {
    if (!nextCursor || loadingMore) return;
    loadingMore = true;
    clearFeedback();
    try {
      const page = await listPage(nextCursor);
      tasks = sortedUnique([...tasks, ...page.records]);
      nextCursor = page.next_cursor;
    } catch (error) {
      report(error);
    } finally {
      loadingMore = false;
    }
  }

  async function add() {
    if (creating) return;
    fieldError = '';
    if (!newTitle.trim()) {
      fieldError = 'Enter a task title.';
      return;
    }
    creating = true;
    clearFeedback();
    try {
      const task = await createTask(newTitle);
      tasks = sortedUnique([...tasks, task]);
      newTitle = '';
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
    openMenuId = null;
    clearFeedback();
    await tick();
    document.getElementById('edit-' + task.id)?.focus();
  }

  async function cancelEdit(id: string) {
    editingId = null;
    editError = '';
    await tick();
    document.getElementById('actions-' + id)?.focus();
  }

  async function saveEdit(task: Task) {
    if (actingId) return;
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
      editingId = null;
      notify('Task updated');
      await tick();
      document.getElementById('actions-' + task.id)?.focus();
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
    if (actingId) return;
    actingId = task.id;
    clearFeedback();
    try {
      const updated = await updateTask(task, { completed: !task.completed });
      tasks = tasks.map((value) => value.id === updated.id ? updated : value);
      notify(updated.completed ? 'Task completed' : 'Task reopened');
    } catch (error) {
      report(error);
    } finally {
      actingId = null;
    }
  }

  async function remove(task: Task) {
    if (actingId) return;
    actingId = task.id;
    clearFeedback();
    try {
      await removeTask(task);
      tasks = tasks.filter((value) => value.id !== task.id);
      confirmingDeleteId = null;
      notify('Task removed');
    } catch (error) {
      report(error);
    } finally {
      actingId = null;
    }
  }

  async function openActions(id: string) {
    openMenuId = openMenuId === id ? null : id;
    if (openMenuId) {
      await tick();
      document.getElementById('task-menu-' + id)?.querySelector('button')?.focus();
    }
  }

  async function closeActions(restoreFocus = false) {
    const id = openMenuId;
    openMenuId = null;
    if (restoreFocus && id) {
      await tick();
      document.getElementById('actions-' + id)?.focus();
    }
  }

  async function confirmDelete(id: string) {
    openMenuId = null;
    confirmingDeleteId = id;
    await tick();
    document.getElementById('delete-confirm-' + id)?.focus();
  }

  async function cancelDelete(id: string) {
    confirmingDeleteId = null;
    await tick();
    document.getElementById('actions-' + id)?.focus();
  }

  function onWindowPointerDown(event: PointerEvent) {
    if (openMenuId && event.target instanceof Element &&
      !event.target.closest('.task-action-area')) {
      openMenuId = null;
    }
  }

  function onWindowKeydown(event: KeyboardEvent) {
    if (event.key !== 'Escape') return;
    if (openMenuId) {
      event.preventDefault();
      void closeActions(true);
    } else if (confirmingDeleteId) {
      event.preventDefault();
      void cancelDelete(confirmingDeleteId);
    } else if (editingId) {
      event.preventDefault();
      void cancelEdit(editingId);
    }
  }
</script>

<svelte:head>
  <title>Todo — Riesel</title>
  <meta name="description" content="Manage tasks with the local Riesel Todo application." />
</svelte:head>

<svelte:window onpointerdown={onWindowPointerDown} onkeydown={onWindowKeydown} />

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
        <p class="eyebrow">YOUR TASKS</p>
        <h1 id="page-title">Tasks</h1>
      </div>
      <p class="page-summary">A clear view of what needs doing.</p>
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
          <button type="submit" class="primary" disabled={creating || actingId !== null}>
            {creating ? 'Adding…' : 'Add task'}
          </button>
        </div>
        {#if fieldError}
          <p class="field-error" id="new-title-error" role="alert">{fieldError}</p>
        {/if}
      </form>

      <div class="toolbar">
        <div class="filters" role="group" aria-label="Filter loaded tasks">
          <button type="button" class:active={filter === 'all'} aria-pressed={filter === 'all'} onclick={() => { filter = 'all'; }}>All</button>
          <button type="button" class:active={filter === 'open'} aria-pressed={filter === 'open'} onclick={() => { filter = 'open'; }}>Open</button>
          <button type="button" class:active={filter === 'done'} aria-pressed={filter === 'done'} onclick={() => { filter = 'done'; }}>Done</button>
        </div>
        <p class="count" aria-label={tasks.length + ' tasks loaded, ' + openCount + ' open in loaded tasks'}>
          {tasks.length} loaded <span aria-hidden="true">·</span> {openCount} open
        </p>
      </div>

      {#if errorMessage}
        <div class="feedback error" role="alert">
          <span>{errorMessage}</span>
          <button type="button" class="text-action" onclick={() => void refresh()}>
            {conflicted ? 'Reload current tasks' : 'Try again'}
          </button>
        </div>
      {/if}

      {#if loading}
        <p class="placeholder" role="status">Loading tasks…</p>
      {:else if visible.length === 0}
        <div class="placeholder">
          <p class="empty-title">{tasks.length === 0 ? 'No tasks yet' : 'Nothing in this view'}</p>
          <p>{tasks.length === 0 ? 'Add your first task above.' : 'Switch filters to see other loaded tasks.'}</p>
        </div>
      {:else}
        <ul class="task-list" aria-label="Tasks">
          {#each visible as task (task.id)}
            <li class="task" class:done={task.completed}>
              <label class="task-check-control">
                <input
                  type="checkbox"
                  class="task-checkbox"
                  checked={task.completed}
                  disabled={actingId !== null}
                  aria-label={'Mark ' + task.title + (task.completed ? ' incomplete' : ' complete')}
                  onchange={() => void toggle(task)}
                />
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
                    <button type="submit" class="small primary" disabled={actingId !== null}>Save</button>
                    <button type="button" class="small" onclick={() => void cancelEdit(task.id)}>Cancel</button>
                    {#if editError}
                      <p class="field-error edit-error" id={'edit-error-' + task.id} role="alert">{editError}</p>
                    {/if}
                  </form>
                {:else}
                  <span class="task-title">{task.title}</span>
                {/if}
              </div>

              {#if editingId !== task.id}
                <div class="task-action-area">
                  {#if confirmingDeleteId === task.id}
                    <div class="delete-confirm" role="group" aria-label={'Confirm removal of ' + task.title}>
                      <span>Remove?</span>
                      <button
                        id={'delete-confirm-' + task.id}
                        type="button"
                        class="small danger"
                        disabled={actingId !== null}
                        onclick={() => void remove(task)}
                      >Delete</button>
                      <button type="button" class="small" onclick={() => void cancelDelete(task.id)}>Cancel</button>
                    </div>
                  {:else}
                    <button
                      id={'actions-' + task.id}
                      type="button"
                      class="task-action-trigger"
                      disabled={actingId !== null}
                      aria-label={'Actions for ' + task.title}
                      aria-expanded={openMenuId === task.id}
                      aria-controls={'task-menu-' + task.id}
                      onclick={() => void openActions(task.id)}
                    ><span aria-hidden="true">⋯</span></button>
                    {#if openMenuId === task.id}
                      <div class="task-menu" id={'task-menu-' + task.id} aria-label={'Actions for ' + task.title}>
                        <button type="button" onclick={() => void startEdit(task)}>Edit task</button>
                        <button type="button" class="menu-danger" onclick={() => void confirmDelete(task.id)}>Remove task</button>
                      </div>
                    {/if}
                  {/if}
                </div>
              {/if}
            </li>
          {/each}
        </ul>
      {/if}

      <div class="panel-footer">
        {#if nextCursor}
          <button type="button" class="secondary" disabled={loadingMore} onclick={() => void loadMore()}>
            {loadingMore ? 'Loading…' : 'Load more tasks'}
          </button>
        {:else if tasks.length > 0}
          <span>All available pages loaded</span>
        {/if}
        <button type="button" class="refresh" disabled={loading} onclick={() => void refresh()}>
          Refresh tasks
        </button>
      </div>
    </section>
    <p class="footnote">Tasks are stored locally. Changes from other clients appear after refresh.</p>
  </main>

  {#if toastMessage}
    <div class="toast-region">
      {#key toastId}
        <Toast message={toastMessage} onDismiss={() => { toastMessage = ''; }} />
      {/key}
    </div>
  {/if}
</div>
