// Ways to sign in without a password: a token a cloud's CLI makes at each connection (AWS
// and Google Cloud IAM, Microsoft Entra ID), Windows' own sign-in to SQL Server, or a Kerberos
// ticket for PostgreSQL. The connection keeps the way in its `auth` option; `platypus-net`
// runs the CLI.
// Types only: the unit tests load this module without the app's.
import type { DriverKind, SavedConnection } from "./api";

export type SignInId = "aws_iam" | "gcp_iam" | "entra_id" | "windows" | "kerberos";

export interface SignIn {
  id: SignInId;
  /** The product's name. */
  name: string;
  /** The databases it signs in to. */
  drivers: DriverKind[];
  /** The command that makes the token, shown to the user; none for Windows and Kerberos. */
  command: ((c: SavedConnection) => string) | null;
}

export const SIGN_INS: SignIn[] = [
  {
    id: "aws_iam",
    name: "AWS IAM",
    drivers: ["postgres", "mysql"],
    command: (c) => {
      const region = c.options?.aws_region ? ` --region ${c.options.aws_region}` : "";
      const profile = c.options?.aws_profile ? ` --profile ${c.options.aws_profile}` : "";
      return `aws rds generate-db-auth-token --hostname ${c.host} --port ${c.port ?? (c.driver === "mysql" ? 3306 : 5432)} --username ${c.user}${region}${profile}`;
    },
  },
  {
    id: "gcp_iam",
    name: "Google Cloud IAM",
    drivers: ["postgres", "mysql"],
    command: () => "gcloud sql generate-login-token",
  },
  {
    id: "entra_id",
    name: "Microsoft Entra ID",
    drivers: ["postgres", "mysql", "sqlserver"],
    command: (c) =>
      `az account get-access-token --resource ${c.driver === "sqlserver" ? "https://database.windows.net/" : "https://ossrdbms-aad.database.windows.net"}`,
  },
  { id: "windows", name: "Windows", drivers: ["sqlserver"], command: null },
  { id: "kerberos", name: "Kerberos", drivers: ["postgres"], command: null },
];

/** The way `c` signs in without a password, `undefined` when it uses one. */
export const signInOf = (c: Pick<SavedConnection, "options">) => SIGN_INS.find((s) => s.id === c.options?.auth);

/** The ways to sign in to `driver`, with `current` kept when it is for another database. */
export const signInsFor = (driver: DriverKind, current?: SignInId) =>
  SIGN_INS.filter((s) => s.drivers.includes(driver) || s.id === current);

/** `c` signing in the way `id` names, or with a password when `null`. */
export function withSignIn(c: SavedConnection, id: SignInId | null): SavedConnection {
  const options = { ...c.options };
  if (id) options.auth = id;
  else delete options.auth;
  // The service name means nothing to the other ways.
  if (id !== "kerberos") delete options.krb_service;
  return { ...c, options };
}
