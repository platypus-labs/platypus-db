/** What the end-to-end tests answer in place of the system, which CDP cannot drive: the folder
 *  picker (`folder`) and the clipboard saved to a file (`clipboard`). Set only with devtools. */
const answers = new Map<string, unknown>();

export const setAnswer = (what: "folder" | "clipboard", value: unknown) => answers.set(what, value);

/** The test's answer to `what`, or `real()`. */
export const answerFor = <T>(what: "folder" | "clipboard", real: () => Promise<T>): Promise<T> =>
  answers.has(what) ? Promise.resolve(answers.get(what) as T) : real();
