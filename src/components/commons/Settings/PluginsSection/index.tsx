import { revealPath } from '~/adapter/shell/shell.client';
import { usePlugins } from '~/usecase/hooks/usePlugins';
import ToggleOption from '~/components/commons/Settings/ToggleOption';
import settingsStyles from '~/components/commons/Settings/styles.module.scss';

import styles from './styles.module.scss';

const PluginsSection = () => {
  const { root, plugins, error, toggle } = usePlugins();

  return (
    <div className={settingsStyles.pane}>
      <div className={settingsStyles.paneHead}>
        <h2 className={settingsStyles.title}>Plugins</h2>
        <p className={settingsStyles.subtitle}>Extend Panorama with themes, fonts and more.</p>
      </div>
      {error && <p className={styles.error}>{error}</p>}
      <div className={settingsStyles.group}>
        <p className={settingsStyles.groupLabel}>Installed</p>
        {plugins.length === 0 ? (
          <p className={settingsStyles.hint}>No plugins installed yet.</p>
        ) : (
          <div className={settingsStyles.options}>
            {plugins.map((plugin) =>
              'error' in plugin ? (
                <p key={plugin.dir} className={styles.error}>
                  {plugin.dir}: {plugin.error}
                </p>
              ) : (
                <ToggleOption
                  key={plugin.dir}
                  checked={plugin.enabled}
                  onToggle={() => toggle(plugin.dir)}
                  label={`${plugin.manifest.name} ${plugin.manifest.version}`}
                  description={plugin.manifest.description ?? plugin.manifest.author ?? ''}
                />
              )
            )}
          </div>
        )}
      </div>
      {root && (
        <div className={settingsStyles.group}>
          <p className={settingsStyles.groupLabel}>Plugins folder</p>
          <p className={settingsStyles.hint}>Each plugin is a folder with a plugin.json. Changes show up right away.</p>
          <span className={styles.path}>{root}</span>
          <button type="button" className={styles.button} onClick={() => revealPath(root)}>
            Open folder
          </button>
        </div>
      )}
    </div>
  );
};

export default PluginsSection;
