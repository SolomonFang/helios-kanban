import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import { Label } from '@/components/ui/label';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { JSONEditor } from '@/components/ui/json-editor';
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group';
import { Check, Loader2 } from 'lucide-react';

import { ExecutorConfigForm } from '@/components/ExecutorConfigForm';
import { AgentConfigPicker } from '@/components/settings/agents/AgentConfigPicker';
import { useProfiles } from '@/hooks/useProfiles';
import { useUserSystem } from '@/components/ConfigProvider';
import { CreateConfigurationDialog } from '@/components/dialogs/settings/CreateConfigurationDialog';
import { DeleteConfigurationDialog } from '@/components/dialogs/settings/DeleteConfigurationDialog';
import type { BaseCodingAgent, ExecutorConfigs } from 'shared/types';

type ExecutorsMap = Record<string, Record<string, Record<string, unknown>>>;
type EditorMode = 'form' | 'json';

export function AgentSettings() {
  const { t } = useTranslation(['settings', 'common']);
  // Use profiles hook for server state
  const {
    profilesContent: serverProfilesContent,
    profilesPath,
    isLoading: profilesLoading,
    isSaving: profilesSaving,
    error: profilesError,
    save: saveProfiles,
  } = useProfiles();

  const { config, updateAndSaveConfig, reloadSystem } = useUserSystem();

  // Local editor state (draft that may differ from server)
  const [localProfilesContent, setLocalProfilesContent] = useState('');
  const [saveError, setSaveError] = useState<string | null>(null);
  const [justSaved, setJustSaved] = useState(false);

  // Form-based editor state
  const [editorMode, setEditorMode] = useState<EditorMode>('form');
  const [selectedExecutorType, setSelectedExecutorType] =
    useState<BaseCodingAgent | null>(null);
  const [selectedConfiguration, setSelectedConfiguration] = useState<
    string | null
  >(null);
  const [localParsedProfiles, setLocalParsedProfiles] =
    useState<ExecutorConfigs | null>(null);
  const [isDirty, setIsDirty] = useState(false);
  const [isJsonValid, setIsJsonValid] = useState(true);

  // Initialize selection from config default, falling back to first executor
  useEffect(() => {
    if (selectedExecutorType) return;
    const configExecutor = config?.executor_profile?.executor;
    if (configExecutor) {
      setSelectedExecutorType(configExecutor);
      setSelectedConfiguration(config?.executor_profile?.variant || 'DEFAULT');
      return;
    }
    if (localParsedProfiles?.executors) {
      const firstExecutor = Object.keys(localParsedProfiles.executors)[0];
      if (firstExecutor) {
        setSelectedExecutorType(firstExecutor as BaseCodingAgent);
      }
    }
  }, [config?.executor_profile, localParsedProfiles, selectedExecutorType]);

  // Sync server state to local state when not dirty
  useEffect(() => {
    if (!isDirty && serverProfilesContent) {
      setLocalProfilesContent(serverProfilesContent);
      // Parse JSON inside effect to avoid object dependency
      try {
        const parsed = JSON.parse(serverProfilesContent);
        setLocalParsedProfiles(parsed);
        setIsJsonValid(true);
      } catch (err) {
        console.error('Failed to parse profiles JSON:', err);
        setLocalParsedProfiles(null);
      }
    }
  }, [serverProfilesContent, isDirty]);

  // Reset local editor state back to the server content
  const discardChanges = () => {
    setIsDirty(false);
    setSaveError(null);
    setLocalProfilesContent(serverProfilesContent);
    try {
      setLocalParsedProfiles(JSON.parse(serverProfilesContent));
      setIsJsonValid(true);
    } catch {
      setLocalParsedProfiles(null);
    }
  };

  // Editor mode switch with unsaved-changes guard
  const handleModeChange = (mode: string) => {
    if (!mode || mode === editorMode) return;
    if (isDirty) {
      if (!window.confirm(t('settings.agents.save.confirmSwitchMode'))) {
        return;
      }
      discardChanges();
    }
    setEditorMode(mode as EditorMode);
  };

  // Sync raw profiles with parsed profiles
  const syncRawProfiles = (profiles: unknown) => {
    setLocalProfilesContent(JSON.stringify(profiles, null, 2));
  };

  // Mark profiles as dirty
  const markDirty = (nextProfiles: unknown) => {
    setLocalParsedProfiles(nextProfiles as ExecutorConfigs);
    syncRawProfiles(nextProfiles);
    setIsDirty(true);
    setIsJsonValid(true);
  };

  // Select an agent and a sensible configuration for it
  const handleSelectExecutor = (executor: BaseCodingAgent) => {
    setSelectedExecutorType(executor);
    const configs = Object.keys(
      localParsedProfiles?.executors?.[executor] || {}
    );
    if (configs.length === 0) {
      setSelectedConfiguration(null);
      return;
    }
    // Prefer the current default variant for this agent, then DEFAULT, then first
    const defaultVariant =
      config?.executor_profile?.executor === executor
        ? (config?.executor_profile?.variant ?? 'DEFAULT')
        : null;
    if (defaultVariant && configs.includes(defaultVariant)) {
      setSelectedConfiguration(defaultVariant);
    } else if (configs.includes('DEFAULT')) {
      setSelectedConfiguration('DEFAULT');
    } else {
      setSelectedConfiguration(configs[0]);
    }
  };

  // Open create dialog
  const handleCreateConfiguration = async (executor: BaseCodingAgent) => {
    try {
      const result = await CreateConfigurationDialog.show({
        executorType: executor,
        existingConfigs: Object.keys(
          localParsedProfiles?.executors?.[executor] || {}
        ),
      });

      if (result.action === 'created' && result.configName) {
        createConfiguration(executor, result.configName, result.cloneFrom);
      }
    } catch {
      // User cancelled - do nothing
    }
  };

  // Create new configuration
  const createConfiguration = (
    executorType: BaseCodingAgent,
    configName: string,
    baseConfig?: string | null
  ) => {
    if (!localParsedProfiles || !localParsedProfiles.executors) return;

    const executorsMap =
      localParsedProfiles.executors as unknown as ExecutorsMap;
    const base =
      baseConfig && executorsMap[executorType]?.[baseConfig]?.[executorType]
        ? executorsMap[executorType][baseConfig][executorType]
        : {};

    const updatedProfiles = {
      ...localParsedProfiles,
      executors: {
        ...localParsedProfiles.executors,
        [executorType]: {
          ...executorsMap[executorType],
          [configName]: {
            [executorType]: base,
          },
        },
      },
    };

    markDirty(updatedProfiles);
    setSelectedExecutorType(executorType);
    setSelectedConfiguration(configName);
  };

  // Open delete dialog
  const handleDeleteConfiguration = async (
    executor: BaseCodingAgent,
    configName: string
  ) => {
    try {
      const result = await DeleteConfigurationDialog.show({
        configName,
        executorType: executor,
      });

      if (result === 'deleted') {
        await deleteConfiguration(executor, configName);
      }
    } catch {
      // User cancelled - do nothing
    }
  };

  // Delete a configuration and save immediately
  const deleteConfiguration = async (
    executorType: BaseCodingAgent,
    configToDelete: string
  ) => {
    if (!localParsedProfiles) return;

    setSaveError(null);

    const executorConfigs = localParsedProfiles.executors[executorType];
    if (!executorConfigs?.[configToDelete]) {
      return;
    }

    // Check if this is the last configuration
    const currentConfigs = Object.keys(executorConfigs);
    if (currentConfigs.length <= 1) {
      return;
    }

    // Remove the configuration from the executor
    const remainingConfigs = { ...executorConfigs };
    delete remainingConfigs[configToDelete];

    const updatedProfiles = {
      ...localParsedProfiles,
      executors: {
        ...localParsedProfiles.executors,
        [executorType]: remainingConfigs,
      },
    };

    const executorsMap = updatedProfiles.executors as unknown as ExecutorsMap;
    // If no configurations left, create a blank DEFAULT (should not happen due to check above)
    if (Object.keys(remainingConfigs).length === 0) {
      executorsMap[executorType] = {
        DEFAULT: { [executorType]: {} },
      };
    }

    try {
      await saveProfiles(JSON.stringify(updatedProfiles, null, 2));
      setLocalParsedProfiles(updatedProfiles);
      setLocalProfilesContent(JSON.stringify(updatedProfiles, null, 2));
      setIsDirty(false);

      // Select another config if we deleted the selected one
      if (
        selectedExecutorType === executorType &&
        selectedConfiguration === configToDelete
      ) {
        const nextConfigs = Object.keys(executorsMap[executorType] || {});
        setSelectedConfiguration(nextConfigs[0] || 'DEFAULT');
      }

      // Refresh global system so deleted configs are removed elsewhere
      reloadSystem();
    } catch (err: unknown) {
      console.error('Failed to save deletion to backend:', err);
      setSaveError(t('settings.agents.errors.deleteFailed'));
    }
  };

  // Make a configuration the default agent profile (saved immediately)
  const handleMakeDefault = async (
    executor: BaseCodingAgent,
    configuration: string
  ) => {
    setSaveError(null);
    try {
      await updateAndSaveConfig({
        executor_profile: {
          executor,
          variant: configuration === 'DEFAULT' ? null : configuration,
        },
      });
      reloadSystem();
    } catch (err) {
      console.error('Error setting default agent:', err);
      setSaveError(t('settings.general.save.error'));
    }
  };

  const handleProfilesChange = (value: string) => {
    setLocalProfilesContent(value);
    setIsDirty(true);

    // Validate JSON on change
    if (value.trim()) {
      try {
        const parsed = JSON.parse(value);
        setLocalParsedProfiles(parsed);
        setIsJsonValid(true);
      } catch {
        // Invalid JSON, keep local content but clear parsed
        setLocalParsedProfiles(null);
        setIsJsonValid(false);
      }
    } else {
      setLocalParsedProfiles(null);
      setIsJsonValid(false);
    }
  };

  // Unified save for both form and JSON editor modes
  const handleSaveProfiles = async () => {
    setSaveError(null);

    const contentToSave =
      editorMode === 'form' && localParsedProfiles
        ? JSON.stringify(localParsedProfiles, null, 2)
        : localProfilesContent;

    try {
      await saveProfiles(contentToSave);
      setIsDirty(false);
      setJustSaved(true);
      setTimeout(() => setJustSaved(false), 3000);

      // Update the local content if using form editor
      if (editorMode === 'form' && localParsedProfiles) {
        setLocalProfilesContent(contentToSave);
      }

      // Refresh global system so new profiles are available elsewhere
      reloadSystem();
    } catch (err: unknown) {
      console.error('Failed to save profiles:', err);
      setSaveError(t('settings.agents.errors.saveFailed'));
    }
  };

  const handleExecutorConfigChange = (
    executorType: string,
    configuration: string,
    formData: unknown
  ) => {
    if (!localParsedProfiles || !localParsedProfiles.executors) return;

    const executorsMap =
      localParsedProfiles.executors as unknown as ExecutorsMap;
    // Update the parsed profiles with the new config
    const updatedProfiles = {
      ...localParsedProfiles,
      executors: {
        ...localParsedProfiles.executors,
        [executorType]: {
          ...executorsMap[executorType],
          [configuration]: {
            [executorType]: formData,
          },
        },
      },
    };

    markDirty(updatedProfiles);
  };

  if (profilesLoading) {
    return (
      <div className="flex items-center justify-center py-8">
        <Loader2 className="h-8 w-8 animate-spin" />
        <span className="ml-2">{t('settings.agents.loading')}</span>
      </div>
    );
  }

  const executorsMap = localParsedProfiles?.executors as unknown as
    | ExecutorsMap
    | undefined;
  const jsonInvalid = editorMode === 'json' && isDirty && !isJsonValid;

  return (
    <div className="space-y-6">
      {!!profilesError && (
        <Alert variant="destructive">
          <AlertDescription>
            {profilesError instanceof Error
              ? profilesError.message
              : String(profilesError)}
          </AlertDescription>
        </Alert>
      )}

      {saveError && (
        <Alert variant="destructive">
          <AlertDescription>{saveError}</AlertDescription>
        </Alert>
      )}

      {jsonInvalid && (
        <Alert variant="destructive">
          <AlertDescription>
            {t('settings.agents.errors.invalidJson')}
          </AlertDescription>
        </Alert>
      )}

      <Card>
        <CardHeader>
          <div className="flex items-start justify-between gap-4">
            <div className="space-y-1.5">
              <CardTitle>{t('settings.agents.title')}</CardTitle>
              <CardDescription>
                {t('settings.agents.description')}
              </CardDescription>
            </div>
            <ToggleGroup
              type="single"
              value={editorMode}
              onValueChange={handleModeChange}
              className="inline-flex gap-1 rounded-md border p-1"
              aria-label={t('settings.agents.editor.modeLabel')}
            >
              <ToggleGroupItem
                value="form"
                size="sm"
                active={editorMode === 'form'}
                aria-label={t('settings.agents.editor.formTab')}
              >
                {t('settings.agents.editor.formTab')}
              </ToggleGroupItem>
              <ToggleGroupItem
                value="json"
                size="sm"
                active={editorMode === 'json'}
                aria-label={t('settings.agents.editor.jsonTab')}
              >
                {t('settings.agents.editor.jsonTab')}
              </ToggleGroupItem>
            </ToggleGroup>
          </div>
        </CardHeader>
        <CardContent className="space-y-4">
          {editorMode === 'form' ? (
            localParsedProfiles?.executors ? (
              <>
                <AgentConfigPicker
                  executors={localParsedProfiles.executors}
                  selectedExecutor={selectedExecutorType}
                  selectedConfiguration={selectedConfiguration}
                  defaultProfile={config?.executor_profile}
                  disabled={profilesSaving}
                  onSelectExecutor={handleSelectExecutor}
                  onSelectConfiguration={setSelectedConfiguration}
                  onCreateConfiguration={handleCreateConfiguration}
                  onMakeDefault={handleMakeDefault}
                  onDeleteConfiguration={handleDeleteConfiguration}
                />

                {selectedExecutorType &&
                  selectedConfiguration &&
                  !!executorsMap?.[selectedExecutorType]?.[
                    selectedConfiguration
                  ]?.[selectedExecutorType] && (
                    <ExecutorConfigForm
                      key={`${selectedExecutorType}-${selectedConfiguration}`}
                      executor={selectedExecutorType}
                      value={
                        (executorsMap[selectedExecutorType][
                          selectedConfiguration
                        ][selectedExecutorType] as Record<string, unknown>) ||
                        {}
                      }
                      onChange={(formData) =>
                        handleExecutorConfigChange(
                          selectedExecutorType,
                          selectedConfiguration,
                          formData
                        )
                      }
                      disabled={profilesSaving}
                    />
                  )}
              </>
            ) : null
          ) : (
            // Raw JSON editor
            <div className="space-y-4">
              <div className="space-y-2">
                <Label htmlFor="profiles-editor">
                  {t('settings.agents.editor.jsonLabel')}
                </Label>
                <JSONEditor
                  id="profiles-editor"
                  placeholder={t('settings.agents.editor.jsonPlaceholder')}
                  value={localProfilesContent}
                  onChange={handleProfilesChange}
                  disabled={profilesLoading}
                  minHeight={300}
                />
              </div>

              {!profilesError && profilesPath && (
                <div className="space-y-2">
                  <p className="text-sm text-muted-foreground">
                    <span className="font-medium">
                      {t('settings.agents.editor.pathLabel')}
                    </span>{' '}
                    <span className="font-mono text-xs">{profilesPath}</span>
                  </p>
                </div>
              )}
            </div>
          )}
        </CardContent>
      </Card>

      {/* Unified sticky save bar for both editor modes */}
      <div className="sticky bottom-0 z-10 bg-background/80 backdrop-blur-sm border-t py-4">
        <div className="flex items-center justify-between">
          <span className="text-sm text-muted-foreground">
            {isDirty ? t('settings.agents.save.unsavedChanges') : ''}
          </span>
          <div className="flex gap-2">
            <Button
              variant="outline"
              onClick={discardChanges}
              disabled={!isDirty || profilesSaving}
            >
              {t('settings.agents.save.discard')}
            </Button>
            <Button
              onClick={handleSaveProfiles}
              disabled={
                !isDirty ||
                profilesSaving ||
                !!profilesError ||
                justSaved ||
                jsonInvalid
              }
            >
              {profilesSaving && (
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              )}
              {justSaved && <Check className="mr-2 h-4 w-4" />}
              {justSaved
                ? t('settings.agents.save.saved')
                : t('settings.agents.save.button')}
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
