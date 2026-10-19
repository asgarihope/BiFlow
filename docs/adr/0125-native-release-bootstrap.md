# ADR 0125: Native release bootstrap on macOS and Windows

Status: Accepted (6.2.74)

The first macOS release attempt stopped before Tauri compilation: the system
Bash 3.2 considers an empty signing-configuration array unset under nounset.
Use guarded array expansion when copying and forwarding optional Tauri flags.
Keep signatures enabled when a key is present, and preserve the unsigned local
build override when it is absent. Exercise the actual build functions with a
stub CLI for signed/unsigned and frontend/reused-frontend combinations. Run
the build-script tests with the system Bash on the native macOS CI runner.

The Windows release attempt received HTTP 503 from Chocolatey's package feed.
Package-manager completion did not prove NSIS was available. Retry installation
up to three times with bounded delays and require a working `makensis /VERSION`
before continuing. Keep release and package dry-run bootstraps aligned.
Native Windows tests simulate both recovery on the third attempt and a persistent
failure, including the feed returning success without installing a compiler.

Do not retry a single job while sibling release builds are active: GitHub may
cancel those jobs when starting the new attempt. Retry failed jobs after the
attempt finishes, or publish a new version after fixing a source failure.
