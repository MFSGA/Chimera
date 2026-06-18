import {
  ProfileQueryResultItem,
  ProfileTemplate,
  useProfile,
  useProfileContent,
} from '@chimera/interface';
import { BaseDialog } from '@chimera/ui';
import { Divider, InputAdornment } from '@mui/material';
import { useAsyncEffect } from 'ahooks';
import { type editor } from 'monaco-editor';
import {
  createContext,
  lazy,
  Suspense,
  use,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import {
  Controller,
  SelectElement,
  TextFieldElement,
  useForm,
} from 'react-hook-form-mui';
import { useLatest } from 'react-use';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';
import { ReadProfile } from './read-profile';
import { ClashProfile, ClashProfileBuilder } from './utils';

const ProfileMonacoViewer = lazy(() => import('./profile-monaco-viewer'));

type RemoteProfileForm = Extract<ClashProfileBuilder, { type: 'remote' }>;

export interface ProfileDialogProps {
  profile?: ProfileQueryResultItem;
  open: boolean;
  onClose: () => void;
}

export type AddProfileContextValue = {
  name: string | null;
  desc: string | null;
  url: string;
};

export const AddProfileContext = createContext<AddProfileContextValue | null>(
  null,
);

export const ProfileDialog = ({
  profile,
  open,
  onClose,
}: ProfileDialogProps) => {
  const { create, patch } = useProfile();

  const contentFn = useProfileContent(profile?.uid ?? '');

  const localProfile = useRef('');
  const addProfileCtx = use(AddProfileContext);
  const [isEdit, setIsEdit] = useState(!!profile);
  const [localProfileMessage] = useState('');

  const { control, watch, handleSubmit, reset, setValue } =
    useForm<ClashProfileBuilder>({
      defaultValues: (profile as ClashProfile) || {
        type: 'remote',
        uid: null,
        name: addProfileCtx?.name || m.profile_new_profile_default_name(),
        desc: addProfileCtx?.desc || '',
        file: null,
        updated: null,
        url: addProfileCtx?.url || '',
        chain: null,
        extra: null,
        option: {
          user_agent: null,
          update_interval: null,
        },
      },
    });

  useEffect(() => {
    if (addProfileCtx) {
      setValue('url', addProfileCtx.url);
      if (addProfileCtx.desc) setValue('desc', addProfileCtx.desc);
      if (addProfileCtx.name) setValue('name', addProfileCtx.name);
    }
  }, [addProfileCtx, setValue]);

  const isRemote = watch('type') === 'remote';

  useEffect(() => {
    setIsEdit(!!profile);
  }, [profile]);

  const commonProps = useMemo(
    () => ({
      autoComplete: 'off',
      autoCorrect: 'off',
      fullWidth: true,
    }),
    [],
  );

  const handleProfileSelected = (content: string) => {
    localProfile.current = content;
  };

  const [editor, setEditor] = useState({
    value: '',
    language: 'yaml',
  });

  const latestEditor = useLatest(editor);

  const editorMarks = useRef<editor.IMarker[]>([]);

  const editorHasError = () =>
    editorMarks.current.length > 0 &&
    editorMarks.current.some((m) => m.severity === 8);

  const onSubmit = handleSubmit(async (form) => {
    if (editorHasError()) {
      message(m.profile_error_before_save(), {
        kind: 'error',
      });
      return;
    }

    const toCreate = async () => {
      if (isRemote) {
        const data = form as RemoteProfileForm;

        await create.mutateAsync({
          type: 'url',
          data: {
            url: data.url ?? '',
            option: data.option
              ? {
                  ...data.option,
                  user_agent: data.option.user_agent ?? null,
                }
              : null,
          },
        });
      } else {
        await create.mutateAsync({
          type: 'manual',
          data: {
            item: form,
            fileData: localProfile.current || ProfileTemplate.profile,
          },
        });
      }
    };

    const toUpdate = async () => {
      const value = latestEditor.current.value;
      const uid = profile?.uid;

      if (!uid) {
        return;
      }

      await contentFn.upsert.mutateAsync(value);

      await patch.mutateAsync({
        uid,
        profile: form,
      });
    };

    try {
      if (isEdit) {
        await toUpdate();
      } else {
        await toCreate();
      }

      setTimeout(() => reset(), 300);

      onClose();
    } catch (err) {
      message(m.profile_error_save_failed() + ' \n' + formatError(err), {
        kind: 'error',
      });
      console.error(err);
    }
  });

  const dialogProps = isEdit && {
    contentStyle: {
      overflow: 'hidden',
      padding: 0,
    },
    full: true,
  };

  const MetaInfo = useMemo(
    () => (
      <div className="flex flex-col gap-4 pt-2 pb-2">
        {!isEdit && (
          <SelectElement
            label={m.profile_type_label()}
            name="type"
            control={control}
            {...commonProps}
            size="small"
            required
            options={[
              {
                id: 'remote',
                label: m.profile_remote_label(),
              },
              {
                id: 'local',
                label: m.profile_local_label(),
              },
            ]}
          />
        )}

        <TextFieldElement
          label={m.profile_form_name_label()}
          name="name"
          control={control}
          size="small"
          fullWidth
          required
        />

        <TextFieldElement
          label={m.profile_form_desc_label()}
          name="desc"
          control={control}
          {...commonProps}
          size="small"
          multiline
        />

        {isRemote && (
          <>
            <TextFieldElement
              label={m.profile_subscription_url_label()}
              name="url"
              control={control}
              {...commonProps}
              size="small"
              multiline
              required
            />

            <TextFieldElement
              label={m.profile_user_agent_label()}
              name="option.user_agent"
              control={control}
              {...commonProps}
              size="small"
              placeholder={`clash-chimera/vdemo`}
            />

            <TextFieldElement
              label={m.profile_update_interval_label()}
              name="option.update_interval"
              control={control}
              {...commonProps}
              size="small"
              type="number"
              slotProps={{
                htmlInput: { min: 0 },
                input: {
                  endAdornment: (
                    <InputAdornment position="end">
                      {m.profile_minutes_unit()}
                    </InputAdornment>
                  ),
                },
              }}
            />
          </>
        )}

        {!isRemote && !isEdit && (
          <>
            <ReadProfile
              key="read_profile"
              onSelected={handleProfileSelected}
            />

            {localProfileMessage && (
              <div className="ml-2 text-red-500">{localProfileMessage}</div>
            )}
            <span className="px-2 text-xs">* {m.profile_upload_hint()}</span>
          </>
        )}
      </div>
    ),
    [commonProps, control, isEdit, isRemote, localProfileMessage],
  );

  useAsyncEffect(async () => {
    if (profile) {
      reset(profile as ClashProfileBuilder);
    }

    if (isEdit) {
      try {
        const value = contentFn.query.data ?? '';
        setEditor((editor) => ({ ...editor, value }));
      } catch (error) {
        console.error(error);
      }
    }
  }, [open]);

  return (
    <BaseDialog
      title={isEdit ? m.profile_update_option_edit() : m.profile_create_title()}
      open={open}
      onClose={() => onClose()}
      onOk={onSubmit}
      divider
      {...dialogProps}
    >
      {isEdit ? (
        <div className="flex h-full">
          <div className="min-w-72 overflow-auto p-4">{MetaInfo}</div>

          <Divider orientation="vertical" />

          <Suspense fallback={null}>
            {open && (
              <ProfileMonacoViewer
                className="w-full"
                readonly={isRemote}
                schemaType="clash"
                value={editor.value}
                onChange={(value) =>
                  setEditor((editor) => ({ ...editor, value }))
                }
                onValidate={(marks) => (editorMarks.current = marks)}
                language={editor.language}
              />
            )}
          </Suspense>
        </div>
      ) : (
        MetaInfo
      )}
    </BaseDialog>
  );
};
