# Upload files

Copy files from your phone or laptop into the folder a tab is working in. Back to the [README](../../README.md). Related: [Workspaces and tabs](./workspaces.md), [Is it safe to use?](../security/safety.md).

## Open the sheet

In an open workspace, tap **Upload files to this tab**. The sheet is titled with the tab's name. Add files with **Choose files…**, drop them on the sheet or paste an image. Nothing starts until you press **Upload**.

## Where the files go

The server looks up the folder the tab is in when you press **Upload** and keeps that folder for the whole upload, even if you change directory in the terminal afterwards. The folder must be inside your home folder on the server; a file never goes anywhere else.

## If a name is taken

For each file, choose **Keep both**, **Replace** or **Skip**. **Same action for the rest** applies your choice to every file in the batch. A name the server would refuse is changed before the upload, and the sheet tells you.

## While it uploads

The sheet shows the progress of each file and of all of them. **Pause** and **Resume** hold and continue a file, **Cancel** stops one, and **Minimize** gets the sheet out of the way. **Cancel upload** stops everything, after you confirm.

If the connection drops, the file shows that it is paused and carries on when the connection is back. If the server no longer keeps it, **Retry** starts it again. A file that fails says why and what to do about it.

## What the server guarantees

Only the owner can upload. A file is created readable and writable only by the user the workspaces run as, is never executable, and replaces an existing file only if you chose **Replace**. A file that is cancelled or fails does not stay behind half written. The server records that an upload happened, never the file's name, path or contents.
