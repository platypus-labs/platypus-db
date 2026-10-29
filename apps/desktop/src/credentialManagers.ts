// Passwords from a password manager or a secrets service: the command line of its CLI, built
// from the reference of the secret. The connection keeps the command (`password_command`),
// and the form recognizes it to show the manager and the reference again.

export type ManagerId = "1password" | "bitwarden" | "vault" | "aws";

export interface CredentialManager {
  id: ManagerId;
  /** The product's name. */
  name: string;
  /** An example reference, shown in the empty field. */
  example: string;
  command: (reference: string) => string;
  /** The reference in `command`, or `null` when it is not this manager's. */
  reference: (command: string) => string | null;
}

/** What the shells would change inside double quotes: the quote itself, `sh`'s `$` and
 *  backtick, `cmd`'s `%`. */
const UNSAFE = /["`$%]/g;

/** `text` between double quotes, which `cmd` and `sh` both read as one argument, without the
 *  characters they would change (the form refuses such a reference before). */
const quoted = (text: string) => `"${text.replace(UNSAFE, "")}"`;

const match = (pattern: RegExp) => (command: string) => pattern.exec(command.trim())?.[1] ?? null;

/** Vault's reference is the secret's path, with `#field` when the password is in another
 *  field than `password`. */
const vaultCommand = (reference: string) => {
  const [path = "", field = "password"] = reference.split("#");
  return `vault kv get -field=${quoted(field || "password")} ${quoted(path)}`;
};

export const MANAGERS: CredentialManager[] = [
  {
    id: "1password",
    name: "1Password",
    example: "op://Work/prod-db/password",
    command: (r) => `op read ${quoted(r)}`,
    reference: match(/^op read "([^"]*)"$/),
  },
  {
    id: "bitwarden",
    name: "Bitwarden",
    example: "prod-db",
    command: (r) => `bw get password ${quoted(r)}`,
    reference: match(/^bw get password "([^"]*)"$/),
  },
  {
    id: "vault",
    name: "HashiCorp Vault",
    example: "secret/prod-db",
    command: vaultCommand,
    reference: (command) => {
      const m = /^vault kv get -field="([^"]*)" "([^"]*)"$/.exec(command.trim());
      if (!m) return null;
      return m[1] === "password" ? m[2]! : `${m[2]}#${m[1]}`;
    },
  },
  {
    id: "aws",
    name: "AWS Secrets Manager",
    example: "prod/db",
    // The secret's text: a database secret is JSON, and its `password` field is taken.
    command: (r) => `aws secretsmanager get-secret-value --secret-id ${quoted(r)} --query SecretString --output text`,
    reference: match(/^aws secretsmanager get-secret-value --secret-id "([^"]*)" --query SecretString --output text$/),
  },
];

/** The manager whose command `command` is, with its reference; `null` for another command. */
export function managerOf(command: string | null | undefined): { manager: CredentialManager; reference: string } | null {
  if (command == null) return null;
  for (const manager of MANAGERS) {
    const reference = manager.reference(command);
    if (reference !== null) return { manager, reference };
  }
  return null;
}

/** Whether `reference` goes into a command as it is: no quote, `$`, backtick or `%`, which
 *  the shells would change. */
export const usableReference = (reference: string) => !reference.match(UNSAFE);
