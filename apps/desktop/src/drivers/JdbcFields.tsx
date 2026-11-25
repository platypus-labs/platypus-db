import { useEffect, useState, type Dispatch, type SetStateAction } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, toCommandError, type JdbcCatalogDriver, type JdbcStatus, type SavedConnection } from "../api";
import { PlatypusProgress } from "../components/PlatypusProgress";
import { formatBytes } from "../format";
import { openExternal } from "../help/external";
import { t } from "../i18n";
import { jdbcOn, refillUrl, type UrlParts } from "../jdbcUrl";

interface Props {
  conn: SavedConnection;
  setConn: Dispatch<SetStateAction<SavedConnection>>;
  /** A Db2 connection: it picks between ODBC and IBM's JDBC driver, and the app makes the URL
   *  from its host, port and database. Otherwise the connection is JDBC all the way: any
   *  driver of the catalog, its version, and the URL. */
  db2?: boolean;
}

const JARS = [{ name: "JAR", extensions: ["jar"] }];

/** The URL parts of `conn` for `driver`'s template. */
const partsOf = (conn: SavedConnection, driver: JdbcCatalogDriver | undefined): UrlParts => ({
  template: driver?.url_template ?? "",
  host: conn.host,
  port: conn.port,
  database: conn.database,
});

/**
 * What a connection through a JDBC driver asks for: the driver the app downloads (its version,
 * with the license shown first when it is not open source), or the user's own jars, and the
 * Java that runs it. The connection's options hold the choice: `jdbc.package` (the driver of
 * the catalog), `jdbc.driver` (its version), `jdbc.jars`, `jdbc.url` and `jdbc.java`.
 */
export function JdbcFields({ conn, setConn, db2 = false }: Props) {
  const [status, setStatus] = useState<JdbcStatus | null>(null);
  const [progress, setProgress] = useState<{ done: number; total: number | null } | null>(null);
  const [error, setError] = useState<string | null>(null);
  // The Java the app finds by itself: the field's placeholder, used unless the connection names one.
  const [javaFound, setJavaFound] = useState<string | null>(null);
  // A version of a driver that is not open source, chosen and not downloaded: the license is
  // shown, and the download waits for the user's button.
  const [pending, setPending] = useState<string | null>(null);
  useEffect(() => {
    api.jdbcStatus().then(setStatus, () => setStatus(null));
    api.javaProgram().then(setJavaFound, () => setJavaFound(null));
  }, []);

  const options = conn.options ?? {};
  const version = options["jdbc.driver"]?.trim() ?? "";
  // Connections saved before the catalog name a version of IBM's driver only.
  const name = options["jdbc.package"]?.trim() || (version ? "db2jcc" : "");
  const files = !name && !!options["jdbc.jars"]?.trim();
  const driver = status?.drivers.find((d) => d.name === (db2 ? "db2jcc" : name));
  // Db2's own driver is chosen in Db2's connection; here the others (and the one a saved connection names).
  const drivers = (status?.drivers ?? []).filter((d) => d.name === name || (d.name === "db2jcc") === db2);

  /** Sets several options at once; an empty value removes one. */
  const patch = (changes: Record<string, string>) =>
    setConn((c) => {
      const next = { ...c.options };
      for (const [key, value] of Object.entries(changes)) {
        if (value.trim()) next[key] = value;
        else delete next[key];
      }
      return { ...c, options: next };
    });

  const pickJars = async (): Promise<string | null> => {
    const picked = await open({ multiple: true, directory: false, filters: JARS });
    return Array.isArray(picked) && picked.length > 0 ? picked.join(";") : null;
  };

  /** Downloads `wanted` of the driver `of`, then chooses it. */
  const install = async (of: string, wanted: string) => {
    setProgress({ done: 0, total: null });
    try {
      setStatus(
        await api.installJdbcDriver(of, wanted, (e) => {
          if (e.kind === "progress") setProgress({ done: e.done, total: e.total });
        }),
      );
      setPending(null);
      // Db2 keeps its jars (a license jar); another driver is the version alone.
      patch({ "jdbc.package": db2 ? "" : of, "jdbc.driver": wanted, ...(db2 ? {} : { "jdbc.jars": "" }) });
    } catch (e) {
      setError(toCommandError(e).message);
    } finally {
      setProgress(null);
    }
  };

  /** Chooses a version: one already downloaded or open source is used (and downloaded) now;
   *  the others wait for the license to be read. */
  const chooseVersion = async (of: string, wanted: string) => {
    setError(null);
    setPending(null);
    const found = status?.drivers.find((d) => d.name === of);
    const known = found?.versions.find((v) => v.version === wanted);
    if (!wanted) patch({ "jdbc.driver": "" });
    else if (known?.installed) patch({ "jdbc.package": db2 ? "" : of, "jdbc.driver": wanted });
    else if (found?.open_source) await install(of, wanted);
    else {
      patch({ "jdbc.driver": "" });
      setPending(wanted);
    }
  };

  /** The Db2 choice: the ODBC driver, a version of IBM's driver, or the user's own jars. */
  const chooseThrough = async (choice: string) => {
    setError(null);
    setPending(null);
    if (choice === "odbc") {
      patch({ "jdbc.driver": "", "jdbc.package": "", "jdbc.jars": "", "jdbc.java": "" });
    } else if (choice === "files") {
      const found = [status?.found_driver, status?.found_license].filter(Boolean).join(";");
      const jars = found || (await pickJars());
      if (jars) patch({ "jdbc.driver": "", "jdbc.package": "", "jdbc.jars": jars });
    } else {
      await chooseVersion("db2jcc", choice.slice(2));
    }
  };

  /** Chooses the driver of a connection that is JDBC all the way: the port and the URL follow
   *  the driver's, unless they were written by hand. */
  const chooseDriver = async (choice: string) => {
    setError(null);
    setPending(null);
    if (choice === "files") {
      const jars = await pickJars();
      if (jars) patch({ "jdbc.driver": "", "jdbc.package": "", "jdbc.jars": jars });
      return;
    }
    const next = status?.drivers.find((d) => d.name === choice);
    setConn((c) => {
      const was = status?.drivers.find((d) => d.name === c.options?.["jdbc.package"]);
      const port = c.port === null || c.port === (was?.default_port ?? null) ? (next?.default_port ?? null) : c.port;
      const url = refillUrl(partsOf(c, was), partsOf({ ...c, port }, next), c.options?.["jdbc.url"] ?? "");
      const kept: Record<string, string> = { ...c.options, "jdbc.package": choice };
      delete kept["jdbc.driver"];
      delete kept["jdbc.jars"];
      if (url) kept["jdbc.url"] = url;
      else delete kept["jdbc.url"];
      return { ...c, port, options: kept };
    });
  };

  /** Changes the host, port or database, and the URL they make with it. */
  const edit = (changes: Partial<Pick<SavedConnection, "host" | "port" | "database">>) =>
    setConn((c) => {
      const next = { ...c, ...changes };
      const url = refillUrl(partsOf(c, driver), partsOf(next, driver), c.options?.["jdbc.url"] ?? "");
      const kept = { ...c.options };
      if (url) kept["jdbc.url"] = url;
      else delete kept["jdbc.url"];
      return { ...next, options: kept };
    });

  const choice = version ? `v:${version}` : files || (db2 && jdbcOn(options)) ? "files" : "odbc";
  const shown = pending ?? version;

  const versionLabel = (v: { version: string; installed: boolean; size: number }) =>
    v.installed ? v.version : t("{version} (to download, {size})", { version: v.version, size: formatBytes(v.size) });

  const license = pending !== null && driver && (
    <div className="notice small jdbc-license" data-notice="jdbc_license">
      <span>{t("{title} is not open source: read its license before downloading it.", { title: driver.title })}</span>
      <button type="button" className="link" onClick={() => void openExternal(driver.license_url)}>
        {t("License: {license}", { license: driver.license })}
      </button>
      <button type="button" data-action="jdbc_download" disabled={progress !== null} onClick={() => void install(driver.name, pending)}>
        {t("Accept and download {version}", { version: pending })}
      </button>
    </div>
  );

  const downloading = (
    <>
      {progress && <PlatypusProgress compact max={progress.total} value={progress.done} label={t("Downloading…")} />}
      {error && <small className="notice error">{error}</small>}
    </>
  );

  const jarsField = (
    <label className="field">
      <span>{version ? t("More jars (a license jar, if you have one)") : t("JDBC driver files")}</span>
      <div className="row tight">
        <input
          value={options["jdbc.jars"] ?? ""}
          onChange={(e) => patch({ "jdbc.jars": e.target.value })}
          data-field="jdbc_jars"
          title={
            db2
              ? t("The jars of IBM's JDBC driver (db2jcc4.jar and, for Db2 for z/OS, its license jar), separated by ;. With them the connection goes through JDBC, which needs Java.")
              : t("The jars of the JDBC driver, separated by ;. With them the connection goes through JDBC, which needs Java.")
          }
        />
        <button
          type="button"
          onClick={async () => {
            const jars = await pickJars();
            if (jars) patch({ "jdbc.jars": jars });
          }}
        >
          {t("Browse…")}
        </button>
      </div>
    </label>
  );

  const javaField = (
    <label className="field">
      <span>{t("Java")}</span>
      <input
        value={options["jdbc.java"] ?? ""}
        onChange={(e) => patch({ "jdbc.java": e.target.value })}
        placeholder={javaFound ?? t("Not found: the java program, or the folder of a Java 11 or later")}
        data-field="jdbc_java"
        title={javaFound !== null ? t("Found by itself (JAVA_HOME, the PATH, DBeaver's)") : t("The java program, or the folder of a Java 11 or later.")}
      />
    </label>
  );

  if (db2) {
    const ibm = status?.drivers.find((d) => d.name === "db2jcc");
    return (
      <>
        <label className="field jdbc-field">
          <span>{t("Connect through")}</span>
          <select
            value={pending ? `v:${pending}` : choice}
            disabled={progress !== null}
            data-field="jdbc_driver"
            onChange={(e) => void chooseThrough(e.target.value)}
            title={t("IBM's JDBC driver connects directly: no client to install, no administrator rights. It needs Java.")}
          >
            <option value="odbc">{t("ODBC driver (IBM client)")}</option>
            {(ibm?.versions ?? []).map((v) => (
              <option key={v.version} value={`v:${v.version}`}>
                {t(v.installed ? "IBM JDBC {version}" : "IBM JDBC {version} (to download)", { version: v.version })}
              </option>
            ))}
            {choice.startsWith("v:") && !ibm?.versions.some((v) => `v:${v.version}` === choice) && (
              <option value={choice}>{t("IBM JDBC {version}", { version: choice.slice(2) })}</option>
            )}
            <option value="files">{t("JDBC driver files…")}</option>
          </select>
        </label>
        {license}
        {downloading}
        {jdbcOn(options) && (
          <>
            {jarsField}
            {javaField}
          </>
        )}
      </>
    );
  }

  return (
    <>
      <div className="field-grid">
        <label className="field">
          <span>{t("Driver")}</span>
          <select
            value={files ? "files" : name}
            disabled={progress !== null}
            data-field="jdbc_package"
            onChange={(e) => void chooseDriver(e.target.value)}
            title={t("The JDBC driver the app downloads from Maven Central, or your own jars. It needs Java.")}
          >
            {drivers.map((d) => (
              <option key={d.name} value={d.name}>
                {d.title}
              </option>
            ))}
            {name && !driver && <option value={name}>{name}</option>}
            <option value="files">{t("JDBC driver files…")}</option>
          </select>
        </label>
        {!files && (
          <label className="field">
            <span>{t("Version")}</span>
            <select
              value={shown}
              disabled={progress !== null}
              data-field="jdbc_version"
              onChange={(e) => void chooseVersion(name, e.target.value)}
            >
              <option value="">{t("Choose a version…")}</option>
              {(driver?.versions ?? []).map((v) => (
                <option key={v.version} value={v.version}>
                  {versionLabel(v)}
                </option>
              ))}
              {version && !driver?.versions.some((v) => v.version === version) && <option value={version}>{version}</option>}
            </select>
          </label>
        )}
      </div>
      {license}
      {downloading}
      <div className="row">
        <label className="field grow">
          <span>{t("Host")}</span>
          <input value={conn.host} data-field="jdbc_host" onChange={(e) => edit({ host: e.target.value })} />
        </label>
        <label className="field port">
          <span>{t("Port")}</span>
          <input
            type="number"
            min={1}
            max={65535}
            value={conn.port ?? ""}
            data-field="jdbc_port"
            onChange={(e) => edit({ port: e.target.value ? Number(e.target.value) : null })}
          />
        </label>
      </div>
      <label className="field">
        <span>{t("Database")}</span>
        <input value={conn.database} data-field="jdbc_database" onChange={(e) => edit({ database: e.target.value })} />
      </label>
      <label className="field">
        <span>{t("JDBC URL")}</span>
        <input
          value={options["jdbc.url"] ?? ""}
          onChange={(e) => patch({ "jdbc.url": e.target.value })}
          placeholder={driver?.url_template ?? "jdbc:…"}
          spellCheck={false}
          autoComplete="off"
          data-field="jdbc_url"
          title={t("Made from the host, port and database; write it by hand to use another form (jdbc:h2:mem:test).")}
        />
      </label>
      {files && jarsField}
      {javaField}
    </>
  );
}
