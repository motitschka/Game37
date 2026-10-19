# The owner's setup is a Config outside the repo; secrets are a separate file

The owner's "hardcoded" setup is a hand-written `~/.config/game37/config.toml`, not code or a profile file in the public repo. The existence of a Config is what skips First-run Setup, so the public code stays generic and new players get setup naturally. The SteamGridDB key lives in its own `~/.config/game37/secrets.toml`, so the Config can be shared or inspected without leaking it.
