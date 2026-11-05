const sql = require('fs').readFileSync('C:/Users/larit/Documents/WOW/launcher/content/sql/custom_item.sql','utf8');
let in_str = false;
for (const c of sql) { if (c === "'") in_str = !in_str; }
console.log('quotes balanced:', !in_str);
console.log('escaped singles count:', (sql.match(/''/g)||[]).length);
const lines = sql.split('\n').filter(l => l.trim() && !l.trimStart().startsWith('--'));
console.log('code lines:', lines.length);
console.log('first 10 bytes:', Buffer.from(sql).subarray(0,10).toString('hex'));
