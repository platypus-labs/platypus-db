import type { DriverKind, InstalledDriver, VendorStatus } from "../api";

/** What the check reads, all of it on this machine: nothing goes to the network. */
export interface DriverLookups {
  /** The databases whose driver is downloaded from the registry (ADBC). */
  downloaded: readonly DriverKind[];
  /** The databases whose vendor library the app installs. */
  vendor: readonly DriverKind[];
  /** The drivers installed. */
  listDrivers: () => Promise<InstalledDriver[]>;
  vendorStatus: (driver: DriverKind) => Promise<Pick<VendorStatus, "package" | "ready">>;
}

/** Whether the app could download a driver for `driver` that is not installed yet. `false`
 *  for a database with nothing to download, where the app installs nothing, and when the
 *  lookup fails: the form must not wait for it nor show an error. */
export async function driverToDownload(driver: DriverKind, lookups: DriverLookups): Promise<boolean> {
  try {
    if (lookups.downloaded.includes(driver)) {
      return !(await lookups.listDrivers()).some((d) => d.name === driver);
    }
    if (lookups.vendor.includes(driver)) {
      const status = await lookups.vendorStatus(driver);
      return status.package !== null && !status.ready;
    }
  } catch {
    // Offline or unreadable: no popup.
  }
  return false;
}
