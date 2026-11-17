
# Riesel — Product Definition

## Purpose

Riesel is a schema-driven application platform for defining,
building, running, and connecting applications through
reusable capabilities.

Developers define application structure, behavior, and
requirements. Riesel provides common infrastructure through
well-defined interfaces.

Applications may combine declarative definitions with
custom code.

## Problem

Application development repeatedly requires similar
infrastructure:

- Data validation and persistence
- Runtime state management
- Application lifecycle management
- Communication between components
- User interfaces
- Process management
- Synchronization

These concerns are often implemented separately for each
application, increasing complexity and limiting reuse.

Riesel aims to provide shared infrastructure without imposing
unnecessary requirements on individual applications.

## Product Model

### Application

A unit of software with defined behavior and lifecycle.

Application logic executes independently of graphical
interfaces.

### Definition

A declarative description of application structure,
behavior, and requirements.

Definitions can describe models, constraints, metadata,
and required capabilities.

A definition is not an executing application.

### Capability

Reusable functionality exposed through a defined contract.

Capabilities may be provided within an application process
or by independent services.

Applications depend on capability contracts rather than
specific implementations.

## Product Requirements

### Schema-Driven Development

Applications can be defined declaratively.

Custom behavior remains possible when application requirements
extend beyond the supported declarative model.

### Headless Execution

Application logic does not require a graphical interface.

Applications can operate without an attached window.

### Independent Interfaces

Native windows execute in separate operating-system processes.

Application and window lifecycles remain independent.

### Reusable Capabilities

Common functionality is provided through reusable contracts.

Applications can use capabilities without implementing their
underlying infrastructure.

### Consistent Interfaces

CLI and programmatic APIs expose equivalent platform operations
with consistent semantics and error behavior.

Direct and managed execution are distinct operations.

### Managed Identity

Riesel manages application, instance, and record identifiers.

Developers are not required to manually assign infrastructure
identifiers for ordinary application operations.

Identifiers remain stable within their defined scope.

### Extensibility

Applications can introduce custom behavior and specialized
capabilities.

The architecture accommodates different execution, data,
interface, and performance requirements.

## Product Interfaces

### Riesel CLI

Command-line access to platform functionality.

### Riesel Rust SDK

Programmatic access to platform capabilities through the
public `riesel` facade.

### Riesel Manager

Application registration, discovery, process supervision,
and coordination.

### Riesel Manager UI

Graphical interface for inspecting and managing applications
and infrastructure.

## Product Scope

Riesel is intended to support:

- Schema-defined applications
- Desktop applications
- Headless services
- Development tools
- Custom interfaces
- Games and real-time applications
- Persistent and runtime state
- Synchronization
- Reusable infrastructure services

Specialized workloads may require specialized capabilities
without changing the shared application model.

## Success Criteria

Riesel is successful when:

1. Applications can be defined and executed through consistent
   declarative and programmatic mechanisms.

2. Common functionality can be reused across independently
   developed applications.

3. Applications can operate without graphical interfaces.

4. Required capabilities can be resolved and accessed through
   defined contracts.

5. CLI and Rust clients expose equivalent operations.

6. Application and infrastructure lifecycles are explicit
   and independently manageable where required.

7. New application types can be supported without replacing
   the platform's foundational concepts.
