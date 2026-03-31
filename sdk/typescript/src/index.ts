/**
 * Juglans TypeScript SDK — run .jg workflows from Node.js.
 *
 * Requires the `juglans` CLI to be installed and available on PATH.
 *
 * @example
 * ```ts
 * import { run, stream, Runner } from '@juglans/sdk';
 *
 * // One-shot
 * const output = await run('main.jg', { query: 'hello' });
 *
 * // Streaming
 * for await (const event of stream('main.jg', { query: 'hello' })) {
 *   if (event.event === 'token') process.stdout.write(event.content);
 * }
 *
 * // Builder
 * const result = await new Runner('main.jg')
 *   .input({ query: 'hello' })
 *   .env({ OPENAI_API_KEY: 'sk-...' })
 *   .run();
 * ```
 */

import { execFile, spawn } from 'child_process';
import { createInterface } from 'readline';

export class JuglansError extends Error {
  constructor(
    public readonly stderr: string,
    public readonly exitCode: number,
  ) {
    super(stderr.trim() || `juglans exited with code ${exitCode}`);
    this.name = 'JuglansError';
  }
}

export interface RunOptions {
  input?: Record<string, unknown>;
  cwd?: string;
  timeout?: number;
  env?: Record<string, string>;
}

/**
 * Run a .jg workflow and return the output as parsed JSON.
 */
export function run(filePath: string, input?: Record<string, unknown>, options?: Omit<RunOptions, 'input'>): Promise<unknown>;
export function run(filePath: string, options?: RunOptions): Promise<unknown>;
export function run(filePath: string, inputOrOptions?: Record<string, unknown> | RunOptions, maybeOptions?: Omit<RunOptions, 'input'>): Promise<unknown> {
  let input: Record<string, unknown> | undefined;
  let options: Omit<RunOptions, 'input'> = {};

  if (maybeOptions) {
    input = inputOrOptions as Record<string, unknown>;
    options = maybeOptions;
  } else if (inputOrOptions && 'input' in inputOrOptions) {
    const { input: i, ...rest } = inputOrOptions as RunOptions;
    input = i;
    options = rest;
  } else {
    input = inputOrOptions as Record<string, unknown> | undefined;
  }

  const args = [filePath, '--output-format', 'json'];
  if (input) {
    args.push('--input', JSON.stringify(input));
  }

  const env = options.env ? { ...process.env, ...options.env } : undefined;

  return new Promise((resolve, reject) => {
    execFile(
      'juglans',
      args,
      {
        cwd: options.cwd,
        timeout: (options.timeout ?? 300) * 1000,
        env,
      },
      (error, stdout, stderr) => {
        if (error) {
          reject(new JuglansError(stderr || error.message, (error as any).code ?? 1));
          return;
        }
        const trimmed = stdout.trim();
        if (!trimmed) {
          resolve(null);
          return;
        }
        try {
          resolve(JSON.parse(trimmed));
        } catch {
          reject(new JuglansError(`Invalid JSON output: ${trimmed}`, 0));
        }
      },
    );
  });
}

export interface SSEEvent {
  event: string;
  [key: string]: unknown;
}

/**
 * Run a .jg workflow in SSE mode, yielding events as objects.
 */
export async function* stream(
  filePath: string,
  options?: RunOptions,
): AsyncGenerator<SSEEvent> {
  const args = [filePath, '--output-format', 'sse'];
  if (options?.input) {
    args.push('--input', JSON.stringify(options.input));
  }

  const env = options?.env ? { ...process.env, ...options.env } : undefined;

  const child = spawn('juglans', args, {
    cwd: options?.cwd,
    env,
    stdio: ['ignore', 'pipe', 'pipe'],
  });

  const rl = createInterface({ input: child.stdout! });
  let stderr = '';
  child.stderr?.on('data', (chunk: Buffer) => {
    stderr += chunk.toString();
  });

  for await (const line of rl) {
    const trimmed = line.trim();
    if (trimmed.startsWith('data: ')) {
      const payload = trimmed.slice(6);
      try {
        yield JSON.parse(payload) as SSEEvent;
      } catch {
        yield { event: 'raw', data: payload };
      }
    }
  }

  const exitCode = await new Promise<number>((resolve) => {
    child.on('close', (code) => resolve(code ?? 0));
  });

  if (exitCode !== 0) {
    throw new JuglansError(stderr, exitCode);
  }
}

/**
 * Builder for configuring and running a Juglans workflow.
 */
export class Runner {
  private _input?: Record<string, unknown>;
  private _cwd?: string;
  private _timeout?: number;
  private _env?: Record<string, string>;

  constructor(private readonly filePath: string) {}

  input(data: Record<string, unknown>): this {
    this._input = data;
    return this;
  }

  cwd(path: string): this {
    this._cwd = path;
    return this;
  }

  timeout(seconds: number): this {
    this._timeout = seconds;
    return this;
  }

  env(vars: Record<string, string>): this {
    this._env = vars;
    return this;
  }

  run(): Promise<unknown> {
    return run(this.filePath, {
      input: this._input,
      cwd: this._cwd,
      timeout: this._timeout,
      env: this._env,
    });
  }

  stream(): AsyncGenerator<SSEEvent> {
    return stream(this.filePath, {
      input: this._input,
      cwd: this._cwd,
      env: this._env,
    });
  }
}
