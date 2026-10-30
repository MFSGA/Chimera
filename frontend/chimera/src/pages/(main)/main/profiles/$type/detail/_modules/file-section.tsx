import {
  commands,
  unwrapResult,
  type ProfileQueryResultItem,
} from '@chimera/interface';
import EditDocumentOutlineRounded from '~icons/material-symbols/edit-document-outline-rounded';
import FolderOpenOutlineRounded from '~icons/material-symbols/folder-open-outline-rounded';
import { useBlockTask } from '@/components/providers/block-task-provider';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';
import { hasMaterializedFile } from '../../../_modules/utils';
import { DetailRow, DetailSection } from './detail-section';
import { useShowInFileManager } from './use-show-in-file-manager';

/** File operations for profiles with an on-disk representation. */
export default function FileSection({
  profile,
}: {
  profile: ProfileQueryResultItem;
}) {
  const task = useBlockTask(`open-profile-editor-${profile.uid}`, async () => {
    try {
      unwrapResult(await commands.createEditorWindow('profile', profile.uid));
    } catch (error) {
      await message(formatError(error), {
        title: m.common_error(),
        kind: 'error',
      });
    }
  });
  const editContent = useLockFn(task.execute);
  const showInFileManager = useShowInFileManager(profile);

  if (!hasMaterializedFile(profile)) return null;

  return (
    <DetailSection title={m.profile_detail_section_file()}>
      <DetailRow
        label={m.profile_detail_edit_content()}
        loading={task.isPending}
        trailing={
          <EditDocumentOutlineRounded
            className="text-on-surface-variant size-5 shrink-0"
            aria-hidden
          />
        }
        onClick={() => void editContent()}
      />
      <DetailRow
        label={m.profile_detail_show_in_file_manager()}
        trailing={
          <FolderOpenOutlineRounded
            className="text-on-surface-variant size-5 shrink-0"
            aria-hidden
          />
        }
        onClick={() => void showInFileManager()}
      />
    </DetailSection>
  );
}
