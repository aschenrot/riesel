
# Todo — Product Definition

## Purpose

Todo is a task management application built using Riesel.

It enables users to create, organize, track, and complete tasks
through a consistent interface.

Todo also serves as a reference application for evaluating
Riesel's platform capabilities.

## Problem

Users need a straightforward way to manage tasks without
unnecessary complexity.

Tasks should remain available across application restarts
and be accessible through supported interfaces.

## Product Model

### Task

A unit of work that can be created, modified, completed,
and removed.

Initial properties:

- Title
- Completion status

Additional properties may be introduced when required by
actual product functionality.

### Task Collection

A set of tasks belonging to the application.

Users can retrieve and interact with their tasks without
manually managing record identifiers.

## Product Requirements

### Task Creation

Users can create tasks by providing a title.

New tasks are incomplete by default.

### Task Retrieval

Users can retrieve and view their existing tasks.

### Task Completion

Users can mark tasks as completed or incomplete.

### Task Editing

Users can modify existing task information.

### Task Deletion

Users can remove tasks they no longer need.

### Persistence

Tasks remain available after the application is closed
and restarted.

### Consistent Access

Todo operations are accessible through supported application
interfaces.

Different interfaces must operate on the same underlying
task data.

### Identity Management

Tasks receive stable identifiers automatically.

Users are not required to provide or manage identifiers.

### Independent Execution

Todo application logic can execute without a graphical window.

The interface can connect to a running application.

## Application Interfaces

### Command Line

Users can manage tasks through commands.

### Graphical Interface

Users can view and manage tasks through a graphical interface.

The graphical interface operates independently of the
headless Todo application.

### Programmatic Interface

Other applications and tools can interact with Todo through
supported operations.

All interfaces share the same task semantics.

## Initial Deliverable

The first functional version supports:

1. Creating tasks
2. Retrieving tasks
3. Marking tasks as completed or incomplete
4. Automatic task identity
5. Persistent task storage
6. Access through CLI and programmatic interfaces

Editing, deletion, graphical interaction, and additional
functionality can follow incrementally.

## Success Criteria

Todo is successful when:

1. Users can create and retrieve tasks reliably.
2. Task changes produce predictable results.
3. Task data survives application restarts.
4. Different interfaces observe consistent task data.
5. Users do not need to manage infrastructure details.
6. The application remains usable without an attached window.
