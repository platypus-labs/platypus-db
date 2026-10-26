// The worker of the mass edit's find and replace (`replaceRunner.ts`): regular expressions
// run here, where a slow one cannot freeze the UI and can be stopped.
import { type ReplaceJob, type ReplaceReply, runReplaceJob } from "./massEdit";

const scope = self as unknown as {
  onmessage: ((e: MessageEvent<ReplaceJob>) => void) | null;
  postMessage(reply: ReplaceReply): void;
};

scope.onmessage = (e) => runReplaceJob(e.data, (reply) => scope.postMessage(reply));
