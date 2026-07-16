# workspace-demo

A two-project workspace: `api` depends on `shared`. From this directory:

```sh
flux workspace status   # see which members are affected by changes
flux workspace build    # build affected members in dependency order
```

Edit a file under `shared/` and rebuild — Flux rebuilds `shared` and `api`.
Edit only `api/` and it rebuilds `api` alone (`shared` stays cached).
