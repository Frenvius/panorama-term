import { isRemote } from '~/adapter/transport';
import { revealPath } from '~/adapter/shell/shell.client';
import { usePlugins } from '~/usecase/hooks/usePlugins';
import { togglePlugin } from '~/usecase/service/plugins.service';
import Group from '~/components/commons/Settings/Group';
import Row from '~/components/commons/Settings/Row';
import Switch from '~/components/commons/Settings/Switch';
import Button from '~/components/commons/Settings/Button';

import styles from './styles.module.scss';

const PluginsSection = () => {
  const { root, plugins, error } = usePlugins();

  const openFolder = () => root && revealPath(root);

  return (
    <>
      {error && <p className={styles.error}>{error}</p>}
      <Group label="Installed">
        {plugins.length === 0 ? (
          <Row label="No plugins installed yet." />
        ) : (
          plugins.map((plugin) => {
            if ('error' in plugin) {
              return (
                <Row key={plugin.dir} label={plugin.dir}>
                  <span className={styles.pluginError}>{plugin.error}</span>
                </Row>
              );
            }
            const handleToggle = () => togglePlugin(plugin.dir);
            return (
              <Row
                key={plugin.dir}
                label={plugin.manifest.name}
                description={plugin.manifest.description ?? plugin.manifest.author ?? ''}
              >
                <span className={styles.version}>{plugin.manifest.version}</span>
                <Switch checked={plugin.enabled} onChange={handleToggle} label={plugin.manifest.name} />
              </Row>
            );
          })
        )}
      </Group>
      {root && (
        <Group label="Folder">
          <Row
            label={root}
            description={isRemote() ? 'Open folder is unavailable on a remote host.' : 'Each plugin is a folder with a plugin.json. Changes show up right away.'}
          >
            <Button disabled={isRemote()} onClick={openFolder}>Open folder</Button>
          </Row>
        </Group>
      )}
    </>
  );
};

export default PluginsSection;
