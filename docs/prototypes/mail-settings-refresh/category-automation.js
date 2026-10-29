// The prototype keeps rule order and model selection in memory for this browser session.
let nextGlobalRuleId = 1;
const globalRules = mainCategories.flatMap(main => main.entries.flatMap(entry => entry.rules.filter(rule => !rule.fixed).map(rule => ({
  ...rule,
  id: `sort-rule-${nextGlobalRuleId++}`,
  targetId: entry.id,
}))));

const ruleList = document.getElementById('automation-rule-list');
const ruleForm = document.getElementById('automation-rule-form');
const ruleFormHome = ruleForm.parentElement;
const newRuleButton = document.getElementById('new-sort-rule');
const ruleField = document.getElementById('sort-rule-field');
const ruleOperator = document.getElementById('sort-rule-operator');
const ruleValueWrap = document.getElementById('sort-rule-value-wrap');
const ruleValue = document.getElementById('sort-rule-value');
const ruleTarget = document.getElementById('sort-rule-target');
const modelEnabled = document.getElementById('category-model-enabled');
const modelSelect = document.getElementById('category-model');

function selectCategoryTab(tab) {
  currentCategoryTab = tab === 'catalog' ? 'catalog' : 'automation';
  document.getElementById('category-catalog').hidden = currentCategoryTab !== 'catalog';
  document.getElementById('category-automation').hidden = currentCategoryTab !== 'automation';
  document.getElementById('new-main-category').hidden = currentCategoryTab !== 'catalog';
  document.querySelectorAll('[data-category-tab]').forEach(button => {
    const active = button.dataset.categoryTab === currentCategoryTab;
    button.classList.toggle('active', active);
    button.setAttribute('aria-selected', String(active));
  });
  if (currentScreen === 'categorization') {
    document.getElementById('page-scroll').scrollTop = 0;
    history.replaceState(null, '', `?screen=categorization&account=${selectedAccount}&tab=${currentCategoryTab}`);
  }
}
document.querySelectorAll('[data-category-tab]').forEach(button => button.addEventListener('click', () => selectCategoryTab(button.dataset.categoryTab)));
selectCategoryTab(currentCategoryTab);

function ruleSummary(rule) {
  if (rule.field === 'Anhang') return rule.operator === 'nicht vorhanden' ? 'Kein Anhang' : 'Hat Anhang';
  const label = ruleFieldChoices.find(choice => choice.key === rule.field)?.label || rule.field;
  return `${label} ${rule.operator || (['Betreff', 'Text'].includes(rule.field) ? 'enthält' : 'ist')} ${rule.value}`;
}
function renderSortRules() {
  if (!ruleForm.hidden) closeGlobalRuleForm();
  ruleList.innerHTML = globalRules.map((rule, index) => {
    const target = findCategory(rule.targetId);
    const parent = mainCategories.find(main => main.entries.includes(target));
    const targetName = target ? `${parent.name} › ${target.name}` : 'Zielkategorie fehlt';
    const actions = `<div class="automation-rule-actions"><button type="button" data-move-rule="up" data-rule-id="${rule.id}" aria-label="Regel nach oben" title="Nach oben" ${index === 0 ? 'disabled' : ''}>↑</button><button type="button" data-move-rule="down" data-rule-id="${rule.id}" aria-label="Regel nach unten" title="Nach unten" ${index === globalRules.length - 1 ? 'disabled' : ''}>↓</button><button class="button category-small-action" type="button" data-edit-global-rule="${rule.id}" aria-expanded="false" aria-controls="automation-rule-form">Bearbeiten</button></div>`;
    return `<div class="automation-rule-row" data-rule-row="${rule.id}"><span class="automation-rule-number">${index + 1}</span><span class="automation-rule-condition">${escapeHtml(ruleSummary(rule))}</span><span class="automation-rule-target" data-color="${parent?.color || 'blue'}">${escapeHtml(targetName)}</span>${actions}</div>`;
  }).join('') || '<p class="automation-empty">Noch keine Regeln. Ohne Treffer übernimmt das gewählte Modell.</p>';
}
renderSortRules();

function fillRuleFields(field = 'Absender', operator, value = '') {
  ruleField.innerHTML = ruleFieldChoices.map(choice => `<option value="${choice.key}">${choice.label}</option>`).join('');
  const choice = ruleFieldChoices.find(item => item.key === field) || ruleFieldChoices[0];
  ruleField.value = choice.key;
  ruleOperator.innerHTML = choice.operators.map(item => `<option value="${item}">${item}</option>`).join('');
  ruleOperator.value = choice.operators.includes(operator) ? operator : choice.operators[0];
  ruleValueWrap.hidden = Boolean(choice.noValue);
  ruleValue.required = !choice.noValue;
  ruleValue.type = choice.number ? 'number' : 'text';
  ruleValue.min = choice.number ? '0' : '';
  ruleValue.step = choice.number ? '0.1' : '';
  ruleValue.placeholder = choice.number ? 'Größe in MB' : 'Wert eingeben';
  ruleValueWrap.querySelector('span').textContent = choice.number ? 'Wert in MB' : 'Wert';
  ruleValue.value = value;
}
function fillRuleTargets(selected = '') {
  ruleTarget.innerHTML = '<option value="">Unterkategorie wählen</option>' + mainCategories.map(main => `<optgroup label="${escapeHtml(main.name)}">${main.entries.map(entry => `<option value="${entry.id}">${escapeHtml(entry.name)}</option>`).join('')}</optgroup>`).join('');
  ruleTarget.value = selected;
}
function openGlobalRuleForm(rule = null) {
  closeGlobalRuleForm();
  const row = rule ? ruleList.querySelector(`[data-rule-row="${rule.id}"]`) : document.createElement('div');
  if (!row) return;
  if (!rule) {
    row.className = 'automation-rule-row automation-rule-new';
    row.innerHTML = '<strong class="automation-rule-condition">Neue Regel</strong>';
    ruleList.prepend(row);
    newRuleButton.setAttribute('aria-expanded', 'true');
  } else {
    const button = row.querySelector('[data-edit-global-rule]');
    button.textContent = 'Schließen';
    button.setAttribute('aria-expanded', 'true');
  }
  row.classList.add('editing');
  ruleForm.dataset.editId = rule?.id || '';
  document.getElementById('remove-sort-rule').hidden = !rule;
  fillRuleFields(rule?.field, rule?.operator, rule?.value);
  fillRuleTargets(rule?.targetId);
  row.append(ruleForm);
  ruleForm.hidden = false;
  row.scrollIntoView({ block: 'nearest' });
  ruleField.focus({ preventScroll: true });
}
function closeGlobalRuleForm() {
  const row = ruleForm.closest('.automation-rule-row');
  if (row) {
    row.classList.remove('editing');
    const button = row.querySelector('[data-edit-global-rule]');
    if (button) { button.textContent = 'Bearbeiten'; button.setAttribute('aria-expanded', 'false'); }
  }
  ruleForm.hidden = true;
  ruleForm.dataset.editId = '';
  ruleFormHome.append(ruleForm);
  ruleList.querySelector('.automation-rule-new')?.remove();
  newRuleButton.setAttribute('aria-expanded', 'false');
}

newRuleButton.addEventListener('click', () => ruleForm.hidden || ruleForm.dataset.editId ? openGlobalRuleForm() : closeGlobalRuleForm());
document.getElementById('cancel-sort-rule').addEventListener('click', closeGlobalRuleForm);
ruleField.addEventListener('change', () => fillRuleFields(ruleField.value));
ruleList.addEventListener('click', event => {
  const button = event.target.closest('button');
  if (!button) return;
  if (button.dataset.editGlobalRule) {
    if (!ruleForm.hidden && ruleForm.dataset.editId === button.dataset.editGlobalRule) closeGlobalRuleForm();
    else openGlobalRuleForm(globalRules.find(rule => rule.id === button.dataset.editGlobalRule));
    return;
  }
  if (button.dataset.moveRule) {
    const index = globalRules.findIndex(rule => rule.id === button.dataset.ruleId);
    const other = index + (button.dataset.moveRule === 'up' ? -1 : 1);
    if (index >= 0 && other >= 0 && other < globalRules.length) {
      [globalRules[index], globalRules[other]] = [globalRules[other], globalRules[index]];
      renderSortRules();
    }
  }
});
ruleForm.addEventListener('submit', event => {
  event.preventDefault();
  const choice = ruleFieldChoices.find(item => item.key === ruleField.value);
  const value = choice.noValue ? '' : ruleValue.value.trim();
  if (!ruleTarget.value || (!choice.noValue && !value)) { ruleForm.reportValidity(); return; }
  const updated = { field: ruleField.value, operator: ruleOperator.value, value, targetId: ruleTarget.value };
  const existing = globalRules.find(rule => rule.id === ruleForm.dataset.editId);
  if (existing) Object.assign(existing, updated);
  else globalRules.push({ id: `sort-rule-${nextGlobalRuleId++}`, ...updated });
  closeGlobalRuleForm();
  renderSortRules();
});
document.getElementById('remove-sort-rule').addEventListener('click', () => {
  const index = globalRules.findIndex(rule => rule.id === ruleForm.dataset.editId);
  if (index >= 0) globalRules.splice(index, 1);
  closeGlobalRuleForm();
  renderSortRules();
});
for (const id of ['main-category-save', 'subcategory-save']) document.getElementById(id).addEventListener('click', renderSortRules);

function updateModelAvailability() {
  modelSelect.disabled = !modelEnabled.checked;
  document.querySelector('.switch-label').textContent = modelEnabled.checked ? 'Aktiv' : 'Aus';
  document.getElementById('automation-model-settings').classList.toggle('disabled', !modelEnabled.checked);
}
modelEnabled.addEventListener('change', updateModelAvailability);
updateModelAvailability();
