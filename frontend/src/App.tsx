import { useEffect } from 'react';
import {
  BrowserRouter,
  Navigate,
  Route,
  Routes,
  useParams,
} from 'react-router-dom';
import { I18nextProvider } from 'react-i18next';
import i18n from '@/i18n';
import { Projects } from '@/pages/Projects';
import { ProjectTasks } from '@/pages/ProjectTasks';
import { FullAttemptLogsPage } from '@/pages/FullAttemptLogs';
import { Migration } from '@/pages/Migration';
import { NormalLayout } from '@/components/layout/NormalLayout';
import { usePostHog } from 'posthog-js/react';
import { useAuth } from '@/hooks';
import { usePreviousPath } from '@/hooks/usePreviousPath';
import { useUiPreferencesScratch } from '@/hooks/useUiPreferencesScratch';

import {
  AgentSettings,
  GeneralSettings,
  McpSettings,
  OrganizationSettings,
  ProjectSettings,
  ReposSettings,
  SettingsLayout,
} from '@/pages/settings/';
import { UserSystemProvider, useUserSystem } from '@/components/ConfigProvider';
import { ThemeProvider } from '@/components/ThemeProvider';
import { SearchProvider } from '@/contexts/SearchContext';

import { HotkeysProvider } from 'react-hotkeys-hook';

import { ProjectProvider } from '@/contexts/ProjectContext';
import { ThemeMode } from 'shared/types';
import { DisclaimerDialog } from '@/components/dialogs/global/DisclaimerDialog';
import { OnboardingDialog } from '@/components/dialogs/global/OnboardingDialog';
import { ReleaseNotesDialog } from '@/components/dialogs/global/ReleaseNotesDialog';
import { ClickedElementsProvider } from '@/contexts/ClickedElementsProvider';

import { LegacyDesignScope } from '@/components/legacy-design/LegacyDesignScope';

/** Legacy-only: redirect old new-UI bookmarks to local-projects. */
function RedirectProjectsToLocal() {
  const { projectId } = useParams();
  return (
    <Navigate
      to={projectId ? `/local-projects/${projectId}` : '/local-projects'}
      replace
    />
  );
}

function AppContent() {
  const { config, analyticsUserId, updateAndSaveConfig } = useUserSystem();
  const posthog = usePostHog();
  const { isSignedIn } = useAuth();

  usePreviousPath();
  useUiPreferencesScratch();

  useEffect(() => {
    if (!posthog || !analyticsUserId) return;

    if (config?.analytics_enabled) {
      posthog.opt_in_capturing();
      posthog.identify(analyticsUserId);
    } else {
      posthog.opt_out_capturing();
    }
  }, [config?.analytics_enabled, analyticsUserId, posthog]);

  useEffect(() => {
    if (!config) return;
    let cancelled = false;

    const showNextStep = async () => {
      if (!config.disclaimer_acknowledged) {
        await DisclaimerDialog.show();
        if (!cancelled) {
          await updateAndSaveConfig({ disclaimer_acknowledged: true });
        }
        DisclaimerDialog.hide();
        return;
      }

      if (!config.onboarding_acknowledged) {
        const result = await OnboardingDialog.show();
        if (!cancelled) {
          await updateAndSaveConfig({
            onboarding_acknowledged: true,
            executor_profile: result.profile,
            editor: result.editor,
          });
        }
        OnboardingDialog.hide();
        return;
      }

      if (config.show_release_notes) {
        await ReleaseNotesDialog.show();
        if (!cancelled) {
          await updateAndSaveConfig({ show_release_notes: false });
        }
        ReleaseNotesDialog.hide();
        return;
      }
    };

    showNextStep();

    return () => {
      cancelled = true;
    };
  }, [config, isSignedIn, updateAndSaveConfig]);

  return (
    <I18nextProvider i18n={i18n}>
      <ThemeProvider initialTheme={config?.theme || ThemeMode.SYSTEM}>
        <SearchProvider>
          <Routes>
            {/* Redirect former new-UI URLs to legacy routes */}
            <Route
              path="/workspaces/*"
              element={<Navigate to="/local-projects" replace />}
            />
            <Route
              path="/projects/:projectId/*"
              element={<RedirectProjectsToLocal />}
            />
            <Route
              path="/projects/:projectId"
              element={<RedirectProjectsToLocal />}
            />
            <Route
              path="/migrate"
              element={<Navigate to="/migration" replace />}
            />

            <Route
              path="/local-projects/:projectId/tasks/:taskId/attempts/:attemptId/full"
              element={
                <LegacyDesignScope>
                  <FullAttemptLogsPage />
                </LegacyDesignScope>
              }
            />

            <Route
              element={
                <LegacyDesignScope>
                  <NormalLayout />
                </LegacyDesignScope>
              }
            >
              <Route path="/" element={<Projects />} />
              <Route path="/local-projects" element={<Projects />} />
              <Route path="/local-projects/:projectId" element={<Projects />} />
              <Route path="/migration" element={<Migration />} />
              <Route
                path="/local-projects/:projectId/tasks"
                element={<ProjectTasks />}
              />
              <Route path="/settings/*" element={<SettingsLayout />}>
                <Route index element={<Navigate to="general" replace />} />
                <Route path="general" element={<GeneralSettings />} />
                <Route path="projects" element={<ProjectSettings />} />
                <Route path="repos" element={<ReposSettings />} />
                <Route
                  path="organizations"
                  element={<OrganizationSettings />}
                />
                <Route path="agents" element={<AgentSettings />} />
                <Route path="mcp" element={<McpSettings />} />
              </Route>
              <Route
                path="/mcp-servers"
                element={<Navigate to="/settings/mcp" replace />}
              />
              <Route
                path="/local-projects/:projectId/tasks/:taskId"
                element={<ProjectTasks />}
              />
              <Route
                path="/local-projects/:projectId/tasks/:taskId/attempts/:attemptId"
                element={<ProjectTasks />}
              />
            </Route>
          </Routes>
        </SearchProvider>
      </ThemeProvider>
    </I18nextProvider>
  );
}

function App() {
  return (
    <BrowserRouter>
      <UserSystemProvider>
        <ClickedElementsProvider>
          <ProjectProvider>
            <HotkeysProvider
              initiallyActiveScopes={['global', 'workspace', 'kanban']}
            >
              <AppContent />
            </HotkeysProvider>
          </ProjectProvider>
        </ClickedElementsProvider>
      </UserSystemProvider>
    </BrowserRouter>
  );
}

export default App;
