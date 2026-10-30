import { toCommandError } from "../api";
import { t } from "../i18n";
import { ShareError } from "./shareQuery";

/** A refusal in the user's language. */
export function shareErrorMessage(e: unknown): string {
  if (!(e instanceof ShareError)) return toCommandError(e).message;
  switch (e.code) {
    case "format":
      return t("This is not a PlatypusDB shared query.");
    case "version":
      return t("This shared query uses a version of the format this PlatypusDB does not read. Update the app to open it.");
    case "too_large":
      return t("The shared query is too large to open.");
    case "shape":
      return t("The shared query is damaged: some of its fields are missing or wrong.");
    case "unsupported":
      return t("This link cannot be read here. Ask for the .platypus file instead.");
    default:
      return t("This is not a valid PlatypusDB query link.");
  }
}
