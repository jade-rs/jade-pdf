// 限定文脈の量を、浮動小数点や文字置換による補正を使わず照合する。
const vulgar=new Map([...'¼½¾⅐⅑⅒⅓⅔⅕⅖⅗⅘⅙⅚⅛⅜⅝⅞↉'].map((c,i)=>[c,[[1,4],[1,2],[3,4],[1,7],[1,9],[1,10],[1,3],[2,3],[1,5],[2,5],[3,5],[4,5],[1,6],[5,6],[1,8],[3,8],[5,8],[7,8],[0,3]][i].map(BigInt)]));
const digit='[0-9]{1,12}',v='['+[...vulgar.keys()].join('')+']',fraction=digit+' *[/⁄] *'+digit;
const number=new RegExp('(?<value>[+−-]?(?:'+digit+' +'+fraction+'|'+digit+' *'+v+'|'+fraction+'|'+v+'|(?:'+digit+')?\\.[0-9]{1,12}|'+digit+'))$','u');
const wordPart=char=>/[\p{L}\p{N}\p{M}_\u200c\u200d\ufeff]/u.test(char);
function gcd(a,b){a=a<0n?-a:a;while(b){[a,b]=[b,a%b];}return a;}
function rational(text){
 text=text.replaceAll('−','-');const sign=text.startsWith('-')?-1n:1n;text=text.replace(/^[+-]/u,'');let numerator,denominator;
 const last=[...text].at(-1);
 if(vulgar.has(last)){const [n,d]=vulgar.get(last);const whole=text.slice(0,-last.length).trim();numerator=BigInt(whole||'0')*d+n;denominator=d;}
 else if(/[\/⁄]/u.test(text)){
  const match=/^(?:([0-9]+) +)?([0-9]+) *\/ *([0-9]+)$/u.exec(text.replaceAll('⁄','/'));if(!match)return null;
  denominator=BigInt(match[3]);if(denominator===0n)return null;numerator=BigInt(match[1]||'0')*denominator+BigInt(match[2]);
 }else{const [whole,decimal='']=text.split('.');numerator=BigInt((whole||'0')+decimal);denominator=10n**BigInt(decimal.length);}
 numerator*=sign;const common=gcd(numerator,denominator);return `${numerator/common}/${denominator/common}`;
}

export function checkQuantityGold(actual,gold,normalize){
 const {context:rawContext,scope,numerator,denominator}=gold;
 if(typeof rawContext!=='string'||!normalize(rawContext)||[...rawContext].length>256
   ||typeof scope!=='string'||!normalize(scope)||[...scope].length>2048
   ||!Number.isSafeInteger(numerator)||numerator<0||numerator>1000000
   ||!Number.isSafeInteger(denominator)||denominator<1||denominator>1000000)
  throw new Error('Quantity context requires bounded text, scope and exact rational integers');
 if(typeof actual!=='string'||actual.length>4000000||[...actual].length>2000000)throw new Error('Quantity context input exceeds the 2-million-scalar budget');
 const text=normalize(actual),context=normalize(rawContext),values=[],lexemes=[];let at=0,count=0;
 for(;;){const found=text.indexOf(context,at);if(found<0)break;at=found+context.length;if(at<text.length&&wordPart(String.fromCodePoint(text.codePointAt(at))))continue;
  if(++count>32)throw new Error('Quantity context exceeds the 32-occurrence budget');
  const prefix=text.slice(Math.max(0,found-64),found).replace(/ +$/u,'');let match=number.exec(prefix);
  if(match&&match.index>0){const previous=[...prefix.slice(0,match.index)].at(-1);if(wordPart(previous)||'./⁄+−-'.includes(previous))match=null;}
  const lexeme=match?match.groups.value:null;lexemes.push(lexeme);values.push(lexeme?rational(lexeme):null);
 }
 const expected=rational(`${numerator}/${denominator}`);
 return {kind:'quantity_context',scope,matches:count===1&&values[0]===expected,expected_value:expected,actual_values:values,context_occurrences:count,numeric_lexemes:lexemes};
}
