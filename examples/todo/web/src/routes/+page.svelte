<script lang="ts">
  import { onMount } from 'svelte';
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
  let editingId = $state<string | null>(null);
  let editDraft = $state('');
  let confirmingDeleteId = $state<string | null>(null);
  let errorMessage = $state('');
  let successMessage = $state('');
  let conflicted = $state(false);

  const visible = $derived(
    tasks.filter((task) =>
      filter === 'all' || (filter === 'done' ? task.completed : !task.completed)
    )
  );
  const openCount = $derived(tasks.filter((task) => !task.completed).length);

  onMount(() => {
    void refresh();
  });

  function clearMessages() {
    errorMessage = '';
    successMessage = '';
    conflicted = false;
  }

  function report(error: unknown) {
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
    clearMessages();
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
    clearMessages();
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
    creating = true;
    clearMessages();
    try {
      const task = await createTask(newTitle);
      tasks = sortedUnique([...tasks, task]);
      newTitle = '';
      successMessage = 'Task created.';
    } catch (error) {
      report(error);
    } finally {
      creating = false;
    }
  }

  function startEdit(task: Task) {
    editingId = task.id;
    editDraft = task.title;
    confirmingDeleteId = null;
    clearMessages();
  }

  async function saveEdit(task: Task) {
    if (actingId) return;
    actingId = task.id;
    clearMessages();
    try {
      const updated = await updateTask(task, { title: editDraft });
      tasks = tasks.map((value) => value.id === updated.id ? updated : value);
      editingId = null;
      successMessage = 'Title updated.';
    } catch (error) {
      report(error);
    } finally {
      actingId = null;
    }
  }

  async function toggle(task: Task) {
    if (actingId) return;
    actingId = task.id;
    clearMessages();
    try {
      const updated = await updateTask(task, { completed: !task.completed });
      tasks = tasks.map((value) => value.id === updated.id ? updated : value);
      successMessage = updated.completed ? 'Task completed.' : 'Task reopened.';
    } catch (error) {
      report(error);
    } finally {
      actingId = null;
    }
  }

  async function remove(task: Task) {
    if (actingId) return;
    actingId = task.id;
    clearMessages();
    try {
      await removeTask(task);
      tasks = tasks.filter((value) => value.id !== task.id);
      confirmingDeleteId = null;
      successMessage = 'Task removed.';
    } catch (error) {
      report(error);
    } finally {
      actingId = null;
    }
  }
</script>

<svelte:head>
  <title>Todo — Riesel</title>
  <meta name="description" content="Manage tasks with the Riesel Todo application." />
</svelte:head>

<div class="app-shell">
  <header class="topbar">
    <div class="brand">
      <span class="brand-mark" aria-hidden="true">R</span>
      <span>Riesel <span class="brand-divider">/</span> Todo</span>
    </div>
    <span class="app-context">Local task manager</span>
  </header>

  <main class="content">
    <section class="intro" aria-labelledby="page-title">
      <p class="eyebrow">TASKS</p>
      <h1 id="page-title">Stay on top of what matters.</h1>
      <p>Keep track of what needs doing, one task at a time.</p>
    </section>

    <section class="panel" aria-label="Task management">
      <form class="new-task" onsubmit={(event) => { event.preventDefault(); void add(); }}>
        <label for="new-title">Add a task</label>
        <div class="new-task-row">
          <input
            id="new-title"
            name="title"
            type="text"
            bind:value={newTitle}
            autocomplete="off"
            placeholder="What needs doing?"
            required
          />
          <button type="submit" class="primary" disabled={creating || actingId !== null}>
            {creating ? 'Adding…' : 'Add task'}
          </button>
        </div>
      </form>

      <div class="toolbar">
        <p class="count">{tasks.length} loaded <span aria-hidden="true">·</span> {openCount} open</p>
        <div class="filters" role="group" aria-label="Filter visible tasks">
          <button type="button" class:active={filter === 'all'} aria-pressed={filter === 'all'} onclick={() => { filter = 'all'; }}>All</button>
          <button type="button" class:active={filter === 'open'} aria-pressed={filter === 'open'} onclick={() => { filter = 'open'; }}>Open</button>
          <button type="button" class:active={filter === 'done'} aria-pressed={filter === 'done'} onclick={() => { filter = 'done'; }}>Done</button>
        </div>
      </div>

      {#if errorMessage}
        <div class="feedback error" role="alert">
          <span>{errorMessage}</span>
          {#if conflicted}
            <button type="button" class="text-action" onclick={() => void refresh()}>
              Reload current tasks
            </button>
          {:else}
            <button type="button" class="text-action" onclick={() => void refresh()}>
              Try again
            </button>
          {/if}
        </div>
      {/if}

      {#if successMessage}
        <p class="feedback success" role="status" aria-live="polite">{successMessage}</p>
      {/if}

      {#if loading}
        <p class="placeholder" role="status">Loading tasks…</p>
      {:else if visible.length === 0}
        <div class="placeholder">
          <p class="empty-title">{tasks.length === 0 ? 'No tasks yet' : 'Nothing in this view'}</p>
          <p>{tasks.length === 0 ? 'Add a task above to get started.' : 'Change the filter to see other tasks.'}</p>
        </div>
      {:else}
        <ul class="task-list" aria-label="Tasks">
          {#each visible as task (task.id)}
            <li class="task" class:done={task.completed}>
              <input
                type="checkbox"
                class="task-checkbox"
                checked={task.completed}
                disabled={actingId !== null}
                aria-label={'Mark ' + task.title + (task.completed ? ' incomplete' : ' complete')}
                onchange={() => void toggle(task)}
              />

              <div class="task-content">
                {#if editingId === task.id}
                  <form class="edit-form" onsubmit={(event) => { event.preventDefault(); void saveEdit(task); }}>
                    <label class="sr-only" for={'edit-' + task.id}>Edit task title</label>
                    <input id={'edit-' + task.id} type="text" bind:value={editDraft} required />
                    <button type="submit" class="small primary" disabled={actingId !== null}>Save</button>
                    <button type="button" class="small" onclick={() => { editingId = null; }}>Cancel</button>
                  </form>
                {:else}
                  <span class="task-title">{task.title}</span>
                {/if}
              </div>

              {#if editingId !== task.id}
                <div class="task-actions">
                  {#if confirmingDeleteId === task.id}
                    <span class="confirm-label">Remove?</span>
                    <button type="button" class="small danger" disabled={actingId !== null} onclick={() => void remove(task)}>Confirm</button>
                    <button type="button" class="small" onclick={() => { confirmingDeleteId = null; }}>Cancel</button>
                  {:else}
                    <button type="button" class="small" disabled={actingId !== null} aria-label={'Edit ' + task.title} onclick={() => startEdit(task)}>Edit</button>
                    <button type="button" class="small" disabled={actingId !== null} aria-label={'Remove ' + task.title} onclick={() => { confirmingDeleteId = task.id; editingId = null; }}>Remove</button>
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
    <p class="footnote">Tasks are stored by your local Riesel backend. Changes from another client appear after refresh.</p>
  </main>
</div>
