-- Workspace restore verifies, snapshots, and rewrites the complete protected
-- dataset. Give both explicit restore phases enough time to finish instead of
-- inheriting the authenticated role's short interactive request timeout.

begin;

alter function timegenie.workspace_restore_prepare(uuid,uuid,uuid,bigint)
  set statement_timeout='600s';
alter function timegenie.workspace_restore_commit(uuid,uuid,uuid,uuid,text,bigint,uuid,boolean)
  set statement_timeout='600s';

notify pgrst,'reload schema';

commit;
