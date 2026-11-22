import type { SelectOption } from '~/components/commons/Settings/Select';

import Select from '~/components/commons/Settings/Select';
import { accountName } from '~/usecase/hooks/useAgentAccounts';
import { useAgyAccountSwitch } from '~/usecase/hooks/useAgyAccountSwitch';

import styles from './styles.module.scss';

interface AccountSwitchProps {
  onSwitched?: () => void;
}

const AccountSwitch = ({ onSwitched }: AccountSwitchProps) => {
  const { list, liveId, switchingId, select } = useAgyAccountSwitch(onSwitched);

  if (list.length < 2) return null;

  const options: SelectOption[] = list.map((a) => ({ value: a.id, label: accountName(a) }));
  const handleChange = (id: string) => void select(id);

  return (
    <div className={styles.section}>
      <span className={styles.title}>{switchingId ? 'Switching...' : 'Account'}</span>
      <Select
        size="sm"
        options={options}
        value={switchingId ?? liveId ?? ''}
        placeholder="Choose account"
        disabled={switchingId !== null}
        onChange={handleChange}
      />
    </div>
  );
};

export default AccountSwitch;
