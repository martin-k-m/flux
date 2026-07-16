# Flux examples

Small, self-contained projects showing Flux across ecosystems. From any example
directory:

```sh
flux validate     # check the .flux
flux build        # run the pipeline
flux test         # run the test step
```

| Example | Stack | `.flux` shows |
| ------- | ----- | ------------- |
| [rust-app](rust-app)     | Rust   | scoped `inputs`, `needs` |
| [node-app](node-app)     | Node   | dependency step + test |
| [python-app](python-app) | Python | dependency step + test |

`flux validate` works without any toolchain installed; `flux build`/`flux test`
run the real commands, so they need the relevant toolchain (cargo / node / python).
