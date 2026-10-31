// A port the OS says is free now, for a server the script starts and configures by environment. A number drawn from
// 30000-59999 at random lands in the kernel's ephemeral range (32768-60999), where the browsers and the runner hold
// outgoing connections, so now and then it was taken and the server died with "Address already in use".
import { createServer } from 'node:net';
import { createSocket } from 'node:dgram';

const claim = (open) => new Promise((resolve, reject) => {
  const socket = open();
  socket.once('error', reject);
  socket.once('listening', () => {
    const { port } = socket.address();
    socket.close(() => resolve(port));
  });
  socket.start();
});

/** A free TCP port on 127.0.0.1 (the servers listen there). */
export const freeTcpPort = () => claim(() => { const s = createServer(); s.start = () => s.listen(0, '127.0.0.1'); return s; });

/** A free UDP port on every address (the servers' WebRTC port is bound on all of them). */
export const freeUdpPort = () => claim(() => { const s = createSocket('udp4'); s.start = () => s.bind(0); return s; });
