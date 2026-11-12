# Model weights

## drone-v1.safetensors

The weights of drone-v1, the key detection network that `euphoria-core` runs
with its `drone` feature. They are compiled into the library with
`include_bytes!`, so nothing is downloaded at run time.

| | |
| --- | --- |
| File | `drone-v1.safetensors`, 441,456 bytes |
| SHA-256 | `aa64343b43e64bdd7b1fb5603cc3c90718d6d6354bd0ff293cf91fc940c43140` |
| Source checkpoint | `drone-v1.pt` from [lvndry/drone](https://github.com/lvndry/drone) and [Hugging Face](https://huggingface.co/lvndry/drone), SHA-256 `dcf8a7a292b7c12076ffff63c8c6550cb09a6c41ce4d000fd7949fc6bb4b452c` |
| Training run | `w1.0-seed13-20261111-185528-lr7fvv4k`: seed 13, 120 epochs, checkpoint chosen at epoch 39 on the validation split |
| Contracts | feature spec `cqt24-8oct-c1-pad4-hop4096-v2`, label schema `tonic-times-two-plus-minor-v1` (both stored in the file's metadata and checked when it loads) |
| Parameters | 109,128, float32, plus the BatchNorm running statistics |
| Licence | MIT, as the drone code and model card state |

The file was made from the checkpoint with drone's exporter, which copies
every tensor unchanged:

```bash
uv run python -m drone.safetensors weights/drone-v1.pt drone-v1.safetensors
```

To check a copy: `shasum -a 256 drone-v1.safetensors`.

### Training data

The network was trained on 1,237 two minute previews from the Beatport EDM
Key dataset and chosen on 126 more from it, whose annotations are published under CC BY-SA 4.0. The audio is
not distributed, here or by drone. The GiantSteps Key dataset (604 previews)
was held out for testing and never used to train or choose the model.
