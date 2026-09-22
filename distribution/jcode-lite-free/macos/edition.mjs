import fs from 'node:fs';
import path from 'node:path';
export default {
  product:'jcode-lite-free', rootName:'JcodeLiteFree', cli:'jcodef', launcher:'jcode-free',
  startOnFresh:false, models:[],
  migrateConfig(home,app,journal) {
    const config=path.join(home,'config.toml');
    if(!fs.existsSync(config)) journal.write(config,fs.readFileSync(path.join(app,'preset/config.toml')));
  },
};
