import { handleDeleteOwner, serveAccountAction, type AccountEnv } from './_lib';

export const onRequestPost = ({ request, env }: { request: Request; env: AccountEnv }): Promise<Response> =>
  serveAccountAction(request, env, 'POST /api/me/delete', 'owner.delete', (owner) => handleDeleteOwner(env.DB, owner));
