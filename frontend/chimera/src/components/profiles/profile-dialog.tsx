import {
  ProfileQueryResultItem,
  RemoteProfile,
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
import { useTranslation } from 'react-i18next';
import { useLatest } from 'react-use';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';
import { ClashProfile, ClashProfileBuilder } from './utils';

const ProfileMonacoViewer = lazy(() => import('./profile-monaco-viewer'));

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
  const { t } = useTranslation();

  const { create, patch } = useProfile();

  const contentFn = useProfileContent(profile?.uid ?? '');

  const addProfileCtx = use(AddProfileContext);
  const [isEdit, setIsEdit] = useState(!!profile);

  const { control, watch, handleSubmit, reset, setValue } =
    useForm<ClashProfileBuilder>({
      defaultValues: (profile as ClashProfile) || {
        type: 'remote',
        name: addProfileCtx?.name || t(`New Profile`),
        desc: addProfileCtx?.desc || '',
        url: addProfileCtx?.url || '',
        option: {
          // user_agent: "",
          with_proxy: false,
          self_proxy: false,
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
      message('Please fix the error before saving', {
        kind: 'error',
      });
      return;
    }

    const toCreate = async () => {
      const data = form as RemoteProfile;

      await create.mutateAsync({
        type: 'url',
        data: {
          url: data.url,
          option: data.option
            ? {
                ...data.option,
                user_agent: data.option.user_agent ?? null,
              }
            : null,
        },
      });
    };

    const toUpdate = async () => {
      const value = latestEditor.current.value;
      await contentFn.upsert.mutateAsync(value);

      await patch.mutateAsync({
        uid: form.uid!,
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
      message('Failed to save profile: \n' + formatError(err), {
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
            label={t('Type')}
            name="type"
            control={control}
            {...commonProps}
            size="small"
            required
            options={[
              {
                id: 'remote',
                label: t('Remote Profile'),
              },
            ]}
          />
        )}

        <TextFieldElement
          label={t('Name')}
          name="name"
          control={control}
          size="small"
          fullWidth
          required
        />

        <TextFieldElement
          label={t('Descriptions')}
          name="desc"
          control={control}
          {...commonProps}
          size="small"
          multiline
        />

        {isRemote && (
          <>
            <TextFieldElement
              label={t('Subscription URL')}
              name="url"
              control={control}
              {...commonProps}
              size="small"
              multiline
              required
            />

            <TextFieldElement
              label={t('User Agent')}
              name="option.user_agent"
              control={control}
              {...commonProps}
              size="small"
              placeholder={`clash-chimera/vdemo`}
            />

            <TextFieldElement
              label={t('Update Interval')}
              name="option.update_interval"
              control={control}
              {...commonProps}
              size="small"
              type="number"
              InputProps={{
                inputProps: { min: 0 },
                endAdornment: (
                  <InputAdornment position="end">{t('minutes')}</InputAdornment>
                ),
              }}
            />
          </>
        )}
      </div>
    ),
    [commonProps, control, isEdit, isRemote, t],
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
      title={isEdit ? t('Edit Profile') : t('Create Profile')}
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
