import React from 'react';
import { Search } from 'lucide-react';

import DockerTab from '~/components/Canvas/Navigator/DockerTab';
import styles from '~/components/ProjectView/panels.module.scss';

const DockerPanel = () => {
  const [query, setQuery] = React.useState('');

  const needle = query.trim().toLowerCase();
  const handleQueryChange = (e: React.ChangeEvent<HTMLInputElement>) => setQuery(e.target.value);

  return (
    <>
      <div className={styles.searchBar}>
        <div className={styles.filter}>
          <Search size={12} strokeWidth={2} />
          <input value={query} onChange={handleQueryChange} placeholder="Filter" />
        </div>
      </div>
      <DockerTab query={needle} />
    </>
  );
};

export default DockerPanel;
