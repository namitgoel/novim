#!/usr/bin/env node

/**
 * Novim CLI - Main entry point
 *
 * A hybrid terminal multiplexer and modal text editor
 * combining the best of tmux and Neovim.
 */

import { Command } from 'commander';
import chalk from 'chalk';
import { NovimCore } from './core.js';

const program = new Command();

program
  .name('novim')
  .version('0.1.0')
  .description('A hybrid terminal multiplexer and modal text editor')
  .argument('[file]', 'File to open')
  .option('-v, --verbose', 'Enable verbose output');

// Info command to show project information
program
  .command('info')
  .description('Show Novim project information')
  .action(() => {
    try {
      const core = new NovimCore();
      const message = core.test();
      console.log(chalk.bold.cyan('Novim v0.1.0'));
      console.log(message);
      console.log();
      console.log(chalk.green('✓ Phase 0: Rust <-> TypeScript communication'));
      console.log(chalk.green('✓ Phase 1: Terminal foundation'));
      console.log(chalk.green('✓ Phase 2: Text buffer with modal editing'));
      console.log(chalk.green('✓ Phase 3: Pane management'));
      console.log(chalk.green('✓ Phase 4: Terminal emulator'));
      console.log(chalk.green('✓ Phase 5: Session persistence'));
      console.log(chalk.green('✓ Phase 6: Command system & polish'));
      console.log();
      console.log(chalk.cyan('Run "novim" to launch the terminal UI'));
    } catch (error) {
      const err = error as Error;
      console.error(chalk.red('✗ Failed to load native module:'));
      console.error(chalk.red(err.message));
      process.exit(1);
    }
  });

// Attach to a saved session
program
  .command('attach <name>')
  .description('Attach to a saved session')
  .action((name: string) => {
    try {
      const core = new NovimCore();
      core.attachSession(name);
    } catch (error) {
      const err = error as Error;
      console.error(chalk.red('Error:'), err.message);
      process.exit(1);
    }
  });

// List saved sessions
program
  .command('list')
  .description('List saved sessions')
  .action(() => {
    try {
      const core = new NovimCore();
      const sessions = core.listSessions();
      if (sessions.length === 0) {
        console.log(chalk.dim('No saved sessions.'));
        console.log(chalk.dim('Save a session with Ctrl+w m inside novim.'));
      } else {
        console.log(chalk.bold('Saved sessions:'));
        for (const name of sessions) {
          console.log(chalk.cyan(`  ${name}`));
        }
        console.log();
        console.log(chalk.dim('Attach with: novim attach <name>'));
      }
    } catch (error) {
      const err = error as Error;
      console.error(chalk.red('Error:'), err.message);
      process.exit(1);
    }
  });

// Test command to verify Rust <-> TypeScript communication
program
  .command('test')
  .description('Test Rust <-> TypeScript communication')
  .action(() => {
    try {
      const core = new NovimCore();
      const message = core.test();
      console.log(chalk.green('✓ Native module loaded successfully!'));
      console.log(chalk.cyan(`Message from Rust: ${message}`));

      const echo = core.echo('Hello from TypeScript!');
      console.log(chalk.cyan(`Echo test: ${echo}`));
    } catch (error) {
      const err = error as Error;
      console.error(chalk.red('✗ Failed to load native module:'));
      console.error(chalk.red(err.message));
      process.exit(1);
    }
  });

// Default action when no command is provided - launch the terminal UI
program.action((file: string | undefined) => {
  try {
    const core = new NovimCore();
    if (file) {
      // File argument → open in editor pane
      core.runTerminalWithFile(file);
    } else {
      // No file → open shell terminal (like tmux)
      core.runTerminalMode();
    }
  } catch (error) {
    const err = error as Error;
    console.error(chalk.red('Error:'), err.message);
    console.error();
    console.error(chalk.yellow('Did you build the Rust code?'));
    console.error(chalk.dim('  cargo build --release'));
    console.error(chalk.dim('  pnpm run copy:native'));
    process.exit(1);
  }
});

program.parse();
