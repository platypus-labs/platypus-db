[← User guide](README.md)

# Work safely: production, read-only, confirmations, masking

Everything here is also in [FEATURES.md](../FEATURES.md#transactions-and-safety).

## Mark a connection as production

1. Edit the connection, open the **Appearance and safety** page.
2. Set the environment to **Production** (the other choices are Development and Test). Production
   asks before changes and starts in manual commit.
3. Optionally pick a color, so the connection is easy to tell apart.

From then on every change on that connection is listed in a dialog titled
"Changes on <name> (production)", with the statements, and runs only after you confirm.
Settings (`Ctrl+,`) › **Safety** › **Ask before changes on production connections** turns this
off for the app. Agents are never more than read only on a production connection.

## Make a connection read-only

1. On the same page, tick **Read-only (warn before modifying data)**.
2. A statement that may modify data is stopped in the editor with "<name> is read-only and this
   may modify data."; some entry points ask "Run anyway?" instead. Import into tables is not
   offered, and agents stay read only.

The lock button of an editor tab makes just that tab read-only, without changing the saved
connection.

The same page has two more boxes: **No exports** (its data goes to no file, other database or
agent) and **No data editing** (rows change only by SQL, not in the grid, imports or generated
data).

## Confirmations

Settings › **Safety** › **Before changes**:

- **UPDATE or DELETE without WHERE** (also `TRUNCATE`): **Ask before running** (the default),
  **Never run** or **Run without asking**. The dialog asks "Change every row?" and says about how
  many rows.
- **Warn when a transaction stays open for (minutes)**: 10 by default, 0 never warns.
- **Add a LIMIT to queries without one (rows)**: 0 (the default) never adds one.
- **Ask before agents change data**: see [Agents](agents.md).

## Undo a change

On by default, **Keep the rows an UPDATE or DELETE changes, to put them back** (Settings ›
Safety › **Lock and backups**) keeps the rows of an `UPDATE` or `DELETE` of one table, up to
10,000, run from the editor. Open the **Transaction log** in the editor's toolbar and click **Restore…**, or
run **Snapshots before changes…** from the command palette, to get the script that puts them back.

## Mask sensitive data

1. Press `Ctrl+Alt+M` (or run **Mask sensitive data** from the command palette): the values of
   sensitive columns show as dots while the screen is shared. Copies keep the real values.
2. Settings › **Safety** › **Masking** sets which columns are sensitive, by name (`*password*`,
   `*token*`, `*email*` and others by default; `*` is any characters) and by type. You can also mark a column in its table's tab.

Agents read the sensitive columns masked too, unless you tick **Show agents the values of
sensitive columns** for that connection in Settings › Agents › **Security**.

## Lock the app

With a master password set (see [Connect](connect.md#master-password)), Settings › **Safety** ›
**Lock the app after this many idle minutes** (0, the default, never locks) locks PlatypusDB and closes the production
connections. **Lock PlatypusDB** in the command palette locks it at once. Enter the master
password to unlock: production connections reconnect when used.
