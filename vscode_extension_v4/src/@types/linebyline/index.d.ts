declare module "linebyline" {
  import { Stream } from "stream";

  function readLine(
    readingObject: string | Stream,
    options?: { maxLineLength?: number; retainBuffer?: boolean },
  ): EventEmitter;
  export = readLine;

  interface EventEmitter {
    on(
      event: "line",
      listener: (line: string, lineCount: number, byteCount: number) => void,
    ): EventEmitter;
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    on(event: "error", listener: (error: any) => void): EventEmitter;
  }
}
