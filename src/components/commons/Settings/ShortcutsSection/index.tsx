import React from 'react';

import { KEYBINDINGS } from '~/usecase/util/keybindings';
import Field, { Input } from '~/components/commons/Settings/Field';
import ShortcutRow from './ShortcutRow';
import styles from './styles.module.scss';

const GROUPS = [...new Set(KEYBINDINGS.map((k) => k.group))];

const ShortcutsSection = () => {
  const [filter, setFilter] = React.useState('');

  const filtered = filter.trim()
    ? KEYBINDINGS.filter(
        (k) =>
          k.label.toLowerCase().includes(filter.toLowerCase()) ||
          k.group.toLowerCase().includes(filter.toLowerCase()) ||
          k.defaultCombo.toLowerCase().includes(filter.toLowerCase())
      )
    : null;

  return (
    <>
      <div className={styles.filterRow}>
        <Field>
          <Input
            value={filter}
            onChange={setFilter}
            placeholder="Filter shortcuts"
            ariaLabel="Filter shortcuts"
          />
        </Field>
      </div>
      {filtered !== null ? (
        filtered.length === 0 ? (
          <div className={styles.empty}>No shortcuts match.</div>
        ) : (
          <div className={styles.card}>
            {filtered.map((k) => (
              <ShortcutRow key={k.id} id={k.id} label={k.label} defaultCombo={k.defaultCombo} />
            ))}
          </div>
        )
      ) : (
        GROUPS.map((group) => (
          <div key={group}>
            <span className={styles.groupLabel}>{group}</span>
            <div className={styles.card}>
              {KEYBINDINGS.filter((k) => k.group === group).map((k) => (
                <ShortcutRow key={k.id} id={k.id} label={k.label} defaultCombo={k.defaultCombo} />
              ))}
            </div>
          </div>
        ))
      )}
    </>
  );
};

export default ShortcutsSection;
