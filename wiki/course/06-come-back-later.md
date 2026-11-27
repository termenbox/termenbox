# Chapter 6: Come back later

In 2 minutes you have closed the app, opened it again and found your work where you left it.

Close the app. Lock the phone. Drive through a tunnel. None of it stops your agent, because the work lives in tmux on your server and your phone is only a window onto it. When the app returns it reconnects by itself and re-attaches, and the screen repaints exactly as it was.

So you never need a "resume" button. Open the same workspace and carry on.

https://github.com/user-attachments/assets/798c495e-185e-488f-9899-4e9ce783eb25

[![Walkthrough: resume after close](../media/resume-after-close-phone-poster.png)](https://termenbox.dev/docs/course/06-come-back-later/)

Watch, 23 s

1. Open a workspace. Tap **shell**.
2. Its screen is live on the server.
3. Lose the signal: a tunnel, a lock screen, a dropped Wi-Fi.
4. The app reconnects by itself and re-attaches.

## If it doesn't work

- **The workspace is marked dormant.** The tmux server on the machine was stopped, for example by a restart. Opening the workspace recreates its tabs as shells with the same names and order, but the work that was running in them is gone. [Workspaces and tabs](../features/workspaces.md#workspaces-and-tabs) explains restore.
- **The app cannot reach the server.** The server may be off, or your network blocks UDP. Check that the server is running, then see [How it connects](../features/connecting.md).

## You can now

- Leave the app, come back, and find the same screen.

That is the course. The Master part, on the terminal, search, several servers and fixing a connection, follows.

Next: [The Master part, in the course index](../course.md).
