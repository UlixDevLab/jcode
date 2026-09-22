import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {pathToFileURL, fileURLToPath} from 'node:url';

const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
test('Free transaction adapter preserves provider choices and installs roles without private routing',async()=>{
  const temp=fs.mkdtempSync(path.join(os.tmpdir(),'free-lifecycle-'));
  try {
    const engine=path.join(temp,'lifecycle');
    fs.cpSync(path.join(root,'../jcode-lite/macos/lifecycle'),engine,{recursive:true});
    fs.copyFileSync(path.join(root,'macos/edition.mjs'),path.join(engine,'edition.mjs'));
    for(const file of fs.readdirSync(engine)) {
      assert.doesNotMatch(fs.readFileSync(path.join(engine,file),'utf8'),/STABLES_API_KEY|router\.legrin-tech\.net|codex\/gpt-|minimax\/M3/);
    }
    const {migrateHome,Journal,aliasLine}=await import(pathToFileURL(path.join(engine,'migrate.mjs')));
    const install=path.join(temp,'JcodeLiteFree'),app=path.join(install,'0.3.0-beta.1');
    for(const dir of ['skills/custom','roles','knowledge-os/templates']) fs.mkdirSync(path.join(app,'preset',dir),{recursive:true});
    fs.writeFileSync(path.join(app,'preset/config.toml'),'[agents]\nmemory_sidecar_enabled = true\n');
    fs.writeFileSync(path.join(app,'preset/swarm-prompt.md'),'Use recipient-configured models.\n');
    fs.writeFileSync(path.join(app,'preset/roles/implement.md'),'Packaged role\n');
    fs.writeFileSync(path.join(app,'preset/skills/custom/SKILL.md'),'Packaged skill\n');
    const home=path.join(install,'home');fs.mkdirSync(home,{recursive:true});
    const config='[provider]\ndefault_provider="recipient"\ndefault_model="chosen-model"\n';
    fs.writeFileSync(path.join(home,'config.toml'),config);
    const journal=new Journal(path.join(temp,'backup')),report=[];
    migrateHome(install,app,null,journal,report);
    assert.equal(fs.readFileSync(path.join(home,'config.toml'),'utf8'),config);
    assert.equal(fs.readFileSync(path.join(home,'roles/implement.md'),'utf8'),'Packaged role\n');
    assert.equal(fs.existsSync(path.join(home,'stables.env')),false);
    const alias=aliasLine(`alias work='${app}/bin/jcode'`,install,temp);
    assert.equal(alias.line,`alias work='"$HOME/.local/bin/jcodef"'`);
    journal.rollback();
    assert.equal(fs.readFileSync(path.join(home,'config.toml'),'utf8'),config);
    assert.equal(fs.existsSync(path.join(home,'roles/implement.md')),false);
  } finally {fs.rmSync(temp,{recursive:true,force:true});}
});
