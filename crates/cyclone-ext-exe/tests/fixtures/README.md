# Fixture HTTP bodies

These JSON documents follow the public exe.dev HTTPS API:

- `ls --json` VM objects are documented at <https://exe.dev/docs/api> (`vm_name`, `https_url`, `ssh_dest`, `status`, …).
- `new --json` is only shown partially in public docs (`{"vm_name":"example-vm"...`). `new_vm.json` uses the same field names as `ls`.

They are **not** live recordings (this repo has no exe.dev credentials). Tests replay them through `MockExecTransport` and a local `POST /exec` server.

No bearer tokens or user secrets belong in this directory.
