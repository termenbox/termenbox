import { handleRegisterServer, serveAccountAction, type AccountEnv } from './_lib';

export const onRequestPost = ({ request, env }: { request: Request; env: AccountEnv }): Promise<Response> =>
  serveAccountAction(request, env, 'POST /api/me/servers', 'owner.server_registered', (owner, body) => handleRegisterServer(env.DB, owner, body, Date.now()));
