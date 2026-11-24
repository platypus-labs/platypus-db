// PlatypusDB's JDBC bridge: a small program the app starts with the JDBC driver of a database
// on its class path, and talks to over its standard input and output. It is what lets the app
// reach a database through a Java driver (Db2 for z/OS through IBM's JCC driver and its license
// jar) without a JVM inside the app.
//
// Build: scripts/jdbc-bridge.mjs (it needs a JDK 11 or later and writes
// platypus-jdbc-bridge.jar, which the Rust side embeds).
// The protocol is documented in src/jdbc/mod.rs.
//
// Requests are lines `<id>\t<COMMAND>\t<argument>...`, each argument base64 of its UTF-8 text.
// Answers are JSON lines. The protocol owns the real standard output; whatever the JDBC driver
// prints is sent to the standard error instead.

import java.io.BufferedReader;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.InputStreamReader;
import java.io.PrintStream;
import java.nio.charset.StandardCharsets;
import java.sql.Connection;
import java.sql.DatabaseMetaData;
import java.sql.DriverManager;
import java.sql.ResultSet;
import java.sql.ResultSetMetaData;
import java.sql.SQLException;
import java.sql.Statement;
import java.sql.Types;
import java.util.Base64;
import java.util.Properties;
import java.util.concurrent.BlockingQueue;
import java.util.concurrent.LinkedBlockingQueue;

public final class Bridge {
    private static PrintStream out;
    private static Connection connection;
    private static volatile Statement current;
    private static volatile boolean cancelled;

    private static final class Request {
        final long id;
        final String command;
        final String[] args;

        Request(long id, String command, String[] args) {
            this.id = id;
            this.command = command;
            this.args = args;
        }
    }

    public static void main(String[] args) throws Exception {
        out = new PrintStream(new FileOutputStream(FileDescriptor.out), false, StandardCharsets.UTF_8);
        System.setOut(System.err);
        BlockingQueue<Request> queue = new LinkedBlockingQueue<>();
        Thread worker = new Thread(() -> work(queue), "bridge-worker");
        worker.start();
        BufferedReader in = new BufferedReader(new InputStreamReader(System.in, StandardCharsets.UTF_8));
        String line;
        while ((line = in.readLine()) != null) {
            String[] parts = line.split("\t", -1);
            if (parts.length < 2) {
                continue;
            }
            if (parts[1].equals("CANCEL")) {
                cancelled = true;
                Statement s = current;
                if (s != null) {
                    try {
                        s.cancel();
                    } catch (SQLException | RuntimeException ignored) {
                        // the statement may have finished meanwhile
                    }
                }
                continue;
            }
            String[] rest = new String[parts.length - 2];
            for (int i = 2; i < parts.length; i++) {
                rest[i - 2] = new String(Base64.getDecoder().decode(parts[i]), StandardCharsets.UTF_8);
            }
            queue.put(new Request(Long.parseLong(parts[0]), parts[1], rest));
        }
        queue.put(new Request(0, "CLOSE", new String[0]));
        worker.join();
    }

    private static void work(BlockingQueue<Request> queue) {
        try {
            while (true) {
                Request r = queue.take();
                if (r.command.equals("CLOSE")) {
                    close();
                    out.flush();
                    return;
                }
                try {
                    handle(r);
                } catch (SQLException e) {
                    error(r.id, e);
                } catch (RuntimeException | LinkageError e) {
                    errorText(r.id, null, e.toString());
                }
            }
        } catch (InterruptedException e) {
            close();
        }
    }

    private static void close() {
        try {
            if (connection != null) {
                connection.close();
            }
        } catch (SQLException ignored) {
            // closing anyway
        }
    }

    private static void handle(Request r) throws SQLException {
        String[] a = r.args;
        switch (r.command) {
            case "CONNECT":
                connect(r.id, a);
                break;
            case "EXEC":
                exec(r.id, a[0], Integer.parseInt(a[1]));
                break;
            case "SCHEMAS":
                try (ResultSet rs = connection.getMetaData().getSchemas()) {
                    rows(r.id, rs, "TABLE_SCHEM");
                }
                break;
            case "TABLES":
                try (ResultSet rs = connection.getMetaData().getTables(null, nullIfEmpty(a[0]), "%", null)) {
                    rows(r.id, rs, "TABLE_SCHEM", "TABLE_NAME", "TABLE_TYPE", "REMARKS");
                }
                break;
            case "COLUMNS":
                try (ResultSet rs = connection.getMetaData().getColumns(null, a[0], nullIfEmpty(a[1]), "%")) {
                    rows(r.id, rs, "TABLE_SCHEM", "TABLE_NAME", "COLUMN_NAME", "DATA_TYPE", "TYPE_NAME",
                        "COLUMN_SIZE", "DECIMAL_DIGITS", "NULLABLE", "COLUMN_DEF", "REMARKS");
                }
                break;
            case "PRIMARY_KEY":
                try (ResultSet rs = connection.getMetaData().getPrimaryKeys(null, a[0], a[1])) {
                    rows(r.id, rs, "COLUMN_NAME", "KEY_SEQ", "PK_NAME");
                }
                break;
            case "FOREIGN_KEYS":
                try (ResultSet rs = connection.getMetaData().getImportedKeys(null, a[0], a[1])) {
                    foreignKeys(r.id, rs);
                }
                break;
            case "REFERENCING":
                try (ResultSet rs = connection.getMetaData().getExportedKeys(null, a[0], a[1])) {
                    foreignKeys(r.id, rs);
                }
                break;
            case "AUTOCOMMIT":
                connection.setAutoCommit(a[0].equals("true"));
                ok(r.id);
                break;
            case "COMMIT":
                connection.commit();
                ok(r.id);
                break;
            case "ROLLBACK":
                connection.rollback();
                ok(r.id);
                break;
            default:
                errorText(r.id, null, "unknown command " + r.command);
        }
    }

    private static String nullIfEmpty(String s) {
        return s == null || s.isEmpty() ? null : s;
    }

    /** CONNECT url user password "key=value" lines, login timeout in seconds. */
    private static void connect(long id, String[] a) throws SQLException {
        Properties props = new Properties();
        if (!a[1].isEmpty()) {
            props.setProperty("user", a[1]);
        }
        if (!a[2].isEmpty()) {
            props.setProperty("password", a[2]);
        }
        for (String pair : a[3].split("\n")) {
            int eq = pair.indexOf('=');
            if (eq > 0) {
                props.setProperty(pair.substring(0, eq).trim(), pair.substring(eq + 1).trim());
            }
        }
        int timeout = a[4].isEmpty() ? 0 : Integer.parseInt(a[4]);
        DriverManager.setLoginTimeout(timeout);
        connection = DriverManager.getConnection(a[0], props);
        DatabaseMetaData meta = connection.getMetaData();
        out.println("{\"id\":" + id + ",\"ok\":true,\"product\":" + quote(meta.getDatabaseProductName())
            + ",\"version\":" + quote(meta.getDatabaseProductVersion()) + "}");
        out.flush();
    }

    private static void exec(long id, String sql, int batchSize) throws SQLException {
        cancelled = false;
        try (Statement st = connection.createStatement()) {
            current = st;
            st.setFetchSize(Math.max(batchSize, 1));
            boolean hasRows = st.execute(sql);
            if (!hasRows) {
                long n = st.getLargeUpdateCount();
                out.println("{\"id\":" + id + ",\"affected\":" + Math.max(n, 0) + "}");
                out.flush();
                return;
            }
            try (ResultSet rs = st.getResultSet()) {
                stream(id, rs, batchSize);
            }
        } finally {
            current = null;
        }
    }

    private static void stream(long id, ResultSet rs, int batchSize) throws SQLException {
        ResultSetMetaData md = rs.getMetaData();
        int n = md.getColumnCount();
        String[] kinds = new String[n];
        boolean[] binary = new boolean[n];
        StringBuilder head = new StringBuilder("{\"id\":" + id + ",\"columns\":[");
        for (int i = 1; i <= n; i++) {
            int type = md.getColumnType(i);
            kinds[i - 1] = kind(type);
            binary[i - 1] = type == Types.BINARY || type == Types.VARBINARY || type == Types.LONGVARBINARY || type == Types.BLOB;
            if (i > 1) {
                head.append(',');
            }
            head.append("{\"name\":").append(quote(md.getColumnLabel(i)))
                .append(",\"kind\":\"").append(kinds[i - 1]).append("\",\"type\":")
                .append(quote(md.getColumnTypeName(i))).append('}');
        }
        out.println(head.append("]}"));
        out.flush();
        StringBuilder batch = new StringBuilder();
        int rows = 0;
        while (!cancelled && rs.next()) {
            batch.append(rows == 0 ? "[" : ",").append('[');
            for (int i = 1; i <= n; i++) {
                if (i > 1) {
                    batch.append(',');
                }
                value(batch, rs, i, kinds[i - 1], binary[i - 1]);
            }
            batch.append(']');
            if (++rows >= batchSize) {
                flushBatch(id, batch);
                rows = 0;
                batch.setLength(0);
            }
        }
        if (rows > 0) {
            flushBatch(id, batch);
        }
        out.println("{\"id\":" + id + ",\"done\":true,\"cancelled\":" + cancelled + "}");
        out.flush();
    }

    private static void flushBatch(long id, StringBuilder batch) {
        out.println("{\"id\":" + id + ",\"batch\":" + batch + "]}");
        out.flush();
    }

    /** `bool`, `int`, `double`, or `text` for everything else (read as text, so exact). */
    private static String kind(int jdbcType) {
        switch (jdbcType) {
            case Types.BIT:
            case Types.BOOLEAN:
                return "bool";
            case Types.TINYINT:
            case Types.SMALLINT:
            case Types.INTEGER:
            case Types.BIGINT:
                return "int";
            case Types.REAL:
            case Types.FLOAT:
            case Types.DOUBLE:
                return "double";
            default:
                return "text";
        }
    }

    private static void value(StringBuilder b, ResultSet rs, int i, String kind, boolean binary) throws SQLException {
        if (kind.equals("bool")) {
            boolean v = rs.getBoolean(i);
            b.append(rs.wasNull() ? "null" : v ? "true" : "false");
        } else if (kind.equals("int")) {
            long v = rs.getLong(i);
            b.append(rs.wasNull() ? "null" : Long.toString(v));
        } else if (kind.equals("double")) {
            double v = rs.getDouble(i);
            if (rs.wasNull()) {
                b.append("null");
            } else if (Double.isFinite(v)) {
                b.append(v);
            } else {
                b.append(quote(Double.toString(v)));
            }
        } else {
            String s;
            if (binary) {
                byte[] bytes = rs.getBytes(i);
                s = bytes == null ? null : hex(bytes);
            } else {
                s = rs.getString(i);
            }
            b.append(s == null ? "null" : quote(s));
        }
    }

    private static String hex(byte[] bytes) {
        StringBuilder s = new StringBuilder(bytes.length * 2);
        for (byte x : bytes) {
            s.append(Character.forDigit((x >> 4) & 15, 16)).append(Character.forDigit(x & 15, 16));
        }
        return s.toString();
    }

    /** A metadata result: the named columns of each row, as text (numbers as numbers). */
    private static void rows(long id, ResultSet rs, String... columns) throws SQLException {
        StringBuilder b = new StringBuilder("{\"id\":" + id + ",\"rows\":[");
        boolean first = true;
        while (rs.next()) {
            b.append(first ? "[" : ",[");
            first = false;
            for (int c = 0; c < columns.length; c++) {
                if (c > 0) {
                    b.append(',');
                }
                Object v = rs.getObject(columns[c]);
                if (v == null) {
                    b.append("null");
                } else if (v instanceof Number) {
                    b.append(((Number) v).longValue());
                } else {
                    b.append(quote(v.toString()));
                }
            }
            b.append(']');
        }
        out.println(b.append("]}"));
        out.flush();
    }

    private static void foreignKeys(long id, ResultSet rs) throws SQLException {
        rows(id, rs, "PKTABLE_SCHEM", "PKTABLE_NAME", "PKCOLUMN_NAME", "FKTABLE_SCHEM", "FKTABLE_NAME",
            "FKCOLUMN_NAME", "KEY_SEQ", "FK_NAME");
    }

    private static void ok(long id) {
        out.println("{\"id\":" + id + ",\"ok\":true}");
        out.flush();
    }

    private static void error(long id, SQLException e) {
        StringBuilder m = new StringBuilder(String.valueOf(e.getMessage()));
        SQLException next = e.getNextException();
        while (next != null && next != e) {
            m.append('\n').append(next.getMessage());
            next = next.getNextException();
        }
        out.println("{\"id\":" + id + ",\"error\":{\"code\":" + quote(e.getSQLState()) + ",\"vendor\":"
            + e.getErrorCode() + ",\"message\":" + quote(m.toString()) + "}}");
        out.flush();
    }

    private static void errorText(long id, String code, String message) {
        out.println("{\"id\":" + id + ",\"error\":{\"code\":" + quote(code) + ",\"vendor\":0,\"message\":"
            + quote(message) + "}}");
        out.flush();
    }

    /** `s` as a JSON string, or `null`. */
    private static String quote(String s) {
        if (s == null) {
            return "null";
        }
        StringBuilder b = new StringBuilder(s.length() + 2).append('"');
        for (int i = 0; i < s.length(); i++) {
            char c = s.charAt(i);
            if (c == '"') {
                b.append("\\\"");
            } else if (c == '\\') {
                b.append("\\\\");
            } else if (c == '\n') {
                b.append("\\n");
            } else if (c == '\r') {
                b.append("\\r");
            } else if (c == '\t') {
                b.append("\\t");
            } else if (c < 0x20) {
                b.append(String.format("\\u%04x", (int) c));
            } else {
                b.append(c);
            }
        }
        return b.append('"').toString();
    }
}
