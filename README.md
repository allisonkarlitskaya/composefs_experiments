# composefs experiments in rust

This is a set of experiments exploring ideas around how to structure an on-disk
[composefs](https://github.com/containers/composefs) repository.

The main consumables here are:

 - a [`Repository`](src/repository.rs) class representing an on-disk composefs
   repository and the operations that can be performed on it.  See the
   [repository format documentation](doc/repository.md).

 - [`cfsctl`](src/bin/cfsctl.rs): a command-line tool for performing operations
   on the repository via the above APIs.

 - (future?) some kind of a system service exposing those APIs to non-root
   users in a safe way.

The `cfsctl mount` command depends on (currently pre-release) Linux 6.12 for
support for directly mounting erofs images without creating loopback devices.

The purpose of this is to iterate fast on some new ideas (without worrying
about breaking existing composefs users) and also as a learning experience (as
my first real Rust project).

Nothing here is currently expected to be useful to anybody at all, and probably
never will be.  If these experiments go well, this code will probably end up
merged in other places.
