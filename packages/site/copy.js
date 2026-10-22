// The only script: the Copy button next to the install command. Without it the command is still selectable text.
const button = document.getElementById('copy');
const cmd = document.getElementById('cmd');
if (button && cmd && navigator.clipboard) {
  button.addEventListener('click', async () => {
    try {
      await navigator.clipboard.writeText(cmd.textContent.trim());
      button.textContent = 'Copied';
    } catch {
      button.textContent = 'Select and copy';
    }
    setTimeout(() => (button.textContent = 'Copy'), 2000);
  });
} else if (button) {
  button.hidden = true;
}
