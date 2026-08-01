# HERMES.md — Gravity Sandbox

## Progetto
Sandbox di gravità N-body in **Bevy 0.19 + Avian 0.7**, compilata in **WASM** e servita su **porta 8081** (testata su iPhone Safari e Mac).

## Vincoli di progetto (NON modificare senza chiedere)
- **UI = Bevy UI nativa** (o altra UI compatibile Bevy scritta in Rust). **VIETATO HTML overlay**.
- Debug **senza console browser**: Davide testa su iPhone Safari/Mac → verificare via `curl` e log, non DevTools.
- Non rimuovere feature funzionanti: se qualcosa non va, **fixare**, non rimuovere.
- Testare i casi limite (es. velocità iniziale zero).

## Workflow
- **Version bump + commit + push ad ogni cambiamento significativo** (version in title + badge ✅/🛑 a ogni build).
- **`/learn` prima di scrivere codice** su API nuove.
- Verificare il build WASM dopo ogni modifica.
- **NON rimuovere le impostazioni di build in `Cargo.toml`** (`[profile.dev] debug = "line-tables-only"`, `incremental = true`): riducono il target dir di ~75%. Il target dir è condiviso in `/home/ubuntu/rust-target/gravity` (config globale `~/.cargo/config.toml` + `.cargo/config.toml` locale, non committare il path assoluto).

## Riferimenti
- API Bevy 0.19 / Avian 0.7: skill `bevy-0.19-development` (ParamSet per query conflittuali, MessageReader/Writer, Time<Virtual> per pause/speed, ecc.).
- Repo GitHub: `dade6/gravity-sandbox`.
