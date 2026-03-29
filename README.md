# Novim

A hybrid terminal multiplexer and modal text editor combining the best of tmux and Neovim.

## Status: Phase 0 Complete ✓

Rust ↔ TypeScript communication is working via Neon FFI.

## Architecture

- **Rust Core** (`novim-core`): Performance-critical operations (terminal rendering, text buffers, terminal emulation)
- **TypeScript Layer** (`novim`): Business logic (CLI, configuration, commands, extensibility)
- **Communication**: Neon FFI for seamless Rust ↔ TypeScript integration

## Building

### Prerequisites

- Rust (latest stable)
- Node.js 18+
- pnpm 8+

### Build Steps

```bash
# Install dependencies
pnpm install

# Build Rust native module
cargo build --release

# Copy native module to TypeScript package
pnpm run copy:native

# Build TypeScript
pnpm run build:ts

# Or build everything at once
pnpm run build
```

## Running

```bash
# Run the CLI
pnpm start

# Or after building
node packages/novim/dist/index.js

# Test Rust <-> TypeScript communication
novim test
```

## Development Phases

- [x] **Phase 0**: Project setup and Rust ↔ TypeScript communication
- [ ] **Phase 1**: Terminal foundation (Ratatui + Crossterm)
- [ ] **Phase 2**: Text buffer with modal editing (Ropey + Normal/Insert modes)
- [ ] **Phase 3**: Pane management (splits, focus switching)
- [ ] **Phase 4**: Terminal emulator (VTE + PTY)
- [ ] **Phase 5**: Session persistence (save/restore sessions)
- [ ] **Phase 6**: Command system & polish

## Project Structure

```
novim/
├── Cargo.toml                    # Rust workspace
├── package.json                  # Root package
├── crates/
│   ├── novim-core/               # Rust native module
│   │   ├── src/
│   │   │   └── lib.rs            # Neon FFI entry point
│   │   └── Cargo.toml
│   └── novim-types/              # Shared types
│       ├── src/
│       │   └── lib.rs
│       └── Cargo.toml
└── packages/
    └── novim/                    # TypeScript package
        ├── src/
        │   ├── index.ts          # CLI entry point
        │   └── core.ts           # Neon bindings wrapper
        ├── package.json
        └── tsconfig.json
```

## License

MIT
