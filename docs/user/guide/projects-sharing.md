[← User guide](README.md)

# Projects and sharing

A project is a group of connections, scripts and open tabs, shown in the rail on the left. The
details are in [FEATURES.md](../FEATURES.md#workspace) and
[FEATURES.md](../FEATURES.md#sharing-and-sync).

## Create a project

1. Run **New project…** from the command palette (`Ctrl+K`).
2. Name it, then choose the connections to show in it. A connection can also be added from its
   own form: **Projects** at the end of the **Appearance and safety** page.
3. `Ctrl+1` shows all connections, `Ctrl+2` to `Ctrl+9` the projects in rail order.

## Share a query

1. In a SQL editor tab, run **Share query…** (tab menu or command palette).
2. Copy the `platypus://…` link, or save a `.platypus` file. It holds the SQL, its title and, if
   you ask, the parameter values; it names the database but never a password.
3. The colleague opens it with **Open a shared link…** or **Open a shared query…**, or by
   dropping the file on the window. The app finds the matching saved connection or offers a new
   one filled in, and shows the query without running it.

## Share a project with a team

1. Right-click the project in the rail and choose **Share project…**, then pick a folder the team
   keeps in Git or on a shared drive. It holds the connections (no credentials) and the scripts.
2. Each colleague clicks **Open shared project…** on the welcome screen, picks the folder, and
   enters their own passwords.
3. After a change in the folder, run **Read the shared folder again** from the project's menu.

Safe by design: a connection that is new from a shared project gives agents **no access** on your
computer until you choose otherwise, and statements the file runs at every connect are shown first
and run only after you approve that exact text.

## Take connections to another computer

On the welcome screen, **Export connections** writes a `.platypusconn` file: pick the connections
and, optionally, a password of 8 characters at least, which encrypts the file and lets it carry the
saved passwords. On the other computer, **Import connections** reads it. Without a password the
file holds no credential.

To keep settings, connections and scripts in step across your computers, use Settings › **Sync**
with a folder in a cloud drive. See [FEATURES.md](../FEATURES.md#sharing-and-sync).

## Scripts and Git

When a project's scripts folder is a Git repository, the Scripts panel shows the branch and the
changes per script, and offers **Commit…**, **Pull** and **Push**. Git must be installed.
