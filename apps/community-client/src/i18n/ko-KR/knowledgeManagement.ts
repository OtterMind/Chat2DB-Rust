export default {
  'knowledgeManagement.title': '지식 관리',
  'knowledgeManagement.nav.terminology': '지식 용어',
  'knowledgeManagement.terminology.description':
    'Chat2DB가 데이터 마이닝 과정에서 대상 설명을 더 잘 이해하도록 지원합니다. 여기에 특정 용어를 입력할 수 있습니다(데이터 소스를 지정할 수 있으며, 지정하지 않으면 전체에 적용됩니다).',
  'knowledgeManagement.terminology.tips':
    '용어 이름: 부실 대출, 용어 설명: 차용인이 약정 기간과 금액에 따라 상환하지 못하는 대출입니다.',
  'knowledgeManagement.nav.businessLogic': '비즈니스 로직',
  'knowledgeManagement.businessLogic.description':
    'Chat2DB가 데이터베이스에 포함된 비즈니스 로직을 더 잘 이해하도록 지원합니다. 여기에 상세 로직을 입력할 수 있습니다(데이터 소스를 지정할 수 있으며, 지정하지 않으면 전체에 적용됩니다).',
  'knowledgeManagement.businessLogic.tips':
    '비즈니스 로직 이름: 수입, 비즈니스 로직 설명: order_item 테이블의 status 필드가 PAY_SUCCESS이며, 주문 금액을 100으로 나눈 뒤 소수점 둘째 자리까지 유지해야 합니다.',
  'knowledgeManagement.nav.caseOptimization': '사례 최적화',
  'knowledgeManagement.caseOptimization.description':
    '위 두 방법을 사용한 뒤에도 결과가 만족스럽지 않다면 모델 출력을 안내할 SQL 템플릿을 구성할 수 있습니다(데이터 소스를 지정할 수 있으며, 지정하지 않으면 전체에 적용됩니다).',
  'knowledgeManagement.caseOptimization.tips':
    '사례 최적화 이름: 최근 30일 신규 사용자, 사례 최적화 내용: SELECT DATE(create_time) AS date, COUNT(*) AS new_users_count FROM user WHERE create_time >= CURDATE() - INTERVAL 30 DAY GROUP BY DATE(create_time) ORDER BY date;',
  'knowledgeManagement.tips.save': '현재 행을 먼저 저장해 주세요',
  'knowledgeManagement.tips.incomplete': '모든 정보를 입력해 주세요',
  'knowledgeManagement.label.knowledgeName': '지식 이름',
  'knowledgeManagement.label.knowledgeContent': '지식 내용',
  'knowledgeManagement.label.boundDataSource': '연결된 데이터 소스',
  'knowledgeManagement.label.businessLogicName': '비즈니스 로직 이름',
  'knowledgeManagement.label.businessLogicContent': '비즈니스 로직 내용',
  'knowledgeManagement.label.caseOptimizationName': '사례 최적화 이름',
  'knowledgeManagement.label.caseOptimizationContent': '사례 최적화 내용',
  'knowledgeManagement.nav.annotationTable': '데이터베이스 테이블 주석',
  'knowledgeManagement.annotationTable.description':
    'Chat2DB는 데이터베이스 테이블 필드에 상세 주석을 추가할 수 있습니다. 이를 통해 AI가 각 필드의 의미를 더 잘 이해하고 데이터 처리 및 분석 정확도를 높일 수 있습니다.',
  'knowledgeManagement.label.batchOperation': '일괄 작업',
  'knowledgeManagement.label.batchImport': '일괄 추가',
  'knowledgeManagement.label.batchExport': '일괄 내보내기',
  'knowledgeManagement.label.batchDelete': '일괄 삭제',
  'knowledgeManagement.label.batchDeleteConfirm': '선택한 레코드 {1}개를 삭제하시겠습니까?',
  'knowledgeManagement.label.batchExportAll': '전체 일괄 내보내기',
  'knowledgeManagement.label.downloadTemplate': '템플릿 다운로드',
  'knowledgeManagement.tips.select': '먼저 작업할 행을 선택해 주세요',
  'knowledgeManagement.tips.importSuccess': '{1}개 항목을 가져왔습니다',
  'knowledgeManagement.tips.exportSuccess': '{1}개 항목을 내보냈습니다',
  'knowledgeManagement.tips.deleteSuccess': '{1}개 항목을 삭제했습니다',
};
