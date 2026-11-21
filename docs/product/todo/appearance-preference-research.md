# Todo appearance preference — reproducible origin research

**Research only, not accepted architecture.** Issue [#106](https://github.com/aschenrot/riesel/issues/106). Investigated 2026-11-21 from `bfdb4e8e67c91ebf7c874fafba72460baf584eda`.

Todo's R2 backend defaults to a randomly assigned loopback port. The system-driven light/dark palette is already delivered; **manual System/Light/Dark choices across restarts remain a separate product decision**.

## Candidate comparison

| Mechanism | Cross-port restart | Owner | Principal drawback |
| --- | --- | --- | --- |
| In-memory override | No | Browser tab | Lost on reload |
| localStorage / IndexedDB | No | Browser origin | Port is part of the origin |
| Host-only non-secret cookie | Yes, under same host/browser profile | Browser host | Also readable and changeable by other same-host services on other ports |
| App-owned settings file or typed model + HTTP API | Potentially yes | Todo installation | Storage/atomicity, concurrent clients, migration and same-origin mutation authorization |
| Stable origin via launcher/Manager | Maybe | Process deployment | Requires an accepted launch/ownership contract; non-default ports can conflict |
| Generic Riesel preference capability | Maybe | Platform | Needs independent multi-app reuse evidence |

The [accepted local HTTP contract](http-local-browser-contract.md) explicitly **forbids cookies**. A non-secret UI-preference cookie could be separately approved, but such a change needs an explicit security exception and cannot serve as authentication. Browser origin isolation is not equivalent to cookie host scope. A browser/installation preference ownership decision is still open.

## Reproducible real-browser experiment

The isolated Playwright test `examples/todo/web/e2e/theme-origin-research.spec.ts` binds two independent ephemeral HTTP servers to numeric `127.0.0.1`. It stores a short-lived SameSite=Strict probe cookie and a localStorage value on one port, navigates to the other and checks cookie visibility, missing localStorage and transmission of a cookie header to the second service. It deletes the cookie from the second port and verifies the first no longer sees it. A second test checks separate browser contexts. A third test closes a real persistent Chromium browser context (ending its process), reopens the same user data directory and visits a different random loopback port; it checks durable cookie visibility, missing cross-port localStorage, and distinct `127.0.0.1` / `localhost` cookie host scopes. No real Todo credentials, API or production settings are used.

**Still not proven:** disabled-cookie fallback, multi-installation collision resolution, cross-browser-engine behavior, exact real Todo backend restart behavior, or safety against malicious same-host processes. The persistent-profile Chromium test verifies browser-process restart with a controlled research server, not a general deployment guarantee. The experiment does not authorize using cookies in Todo.

Sources: https://www.rfc-editor.org/rfc/rfc6265#section-8.5 and https://developer.mozilla.org/en-US/docs/Web/CSS/Reference/Values/color_value/light-dark .
